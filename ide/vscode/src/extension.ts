import * as vscode from 'vscode';

interface SisyphusConfig {
  baseUrl: string;
  authToken: string;
}

function getConfig(): SisyphusConfig {
  const cfg = vscode.workspace.getConfiguration('sisyphus');
  return {
    baseUrl: cfg.get<string>('baseUrl', 'http://localhost:8080'),
    authToken: cfg.get<string>('authToken', ''),
  };
}

async function apiRequest<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const config = getConfig();
  const url = `${config.baseUrl}${path}`;

  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
  };
  if (config.authToken) {
    headers['Authorization'] = `Bearer ${config.authToken}`;
  }

  const response = await fetch(url, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!response.ok) {
    throw new Error(`API error ${response.status}: ${await response.text()}`);
  }

  return (await response.json()) as T;
}

interface HealthResponse {
  status: string;
  version: string;
  uptime_secs: number;
}

export function activate(context: vscode.ExtensionContext) {
  // ── Status Command ────────────────────────────
  const statusCmd = vscode.commands.registerCommand(
    'sisyphus.status',
    async () => {
      try {
        const health = await apiRequest<HealthResponse>('GET', '/health');
        const sessions = await apiRequest<any[]>('GET', '/api/v1/sessions');

        const panel = vscode.window.createWebviewPanel(
          'sisyphusStatus',
          'Sisyphus Status',
          vscode.ViewColumn.One,
          {},
        );

        panel.webview.html = getWebviewContent(health, sessions.length);

        vscode.window.showInformationMessage(
          `Sisyphus v${health.version} — ${health.status} (uptime: ${health.uptime_secs}s)`,
        );
      } catch (e: any) {
        vscode.window.showErrorMessage(
          `Cannot connect to Sisyphus: ${e.message}`,
        );
      }
    },
  );

  // ── New Session Command ──────────────────────
  const newSessionCmd = vscode.commands.registerCommand(
    'sisyphus.newSession',
    async () => {
      const name = await vscode.window.showInputBox({
        prompt: 'Session name',
        placeHolder: 'my-session',
      });
      if (!name) return;

      try {
        const session = await apiRequest<any>('POST', '/api/v1/sessions', {
          name,
        });
        vscode.window.showInformationMessage(
          `Session created: ${session.id}`,
        );
      } catch (e: any) {
        vscode.window.showErrorMessage(`Failed: ${e.message}`);
      }
    },
  );

  // ── Search Memory Command ───────────────────
  const searchCmd = vscode.commands.registerCommand(
    'sisyphus.searchMemory',
    async () => {
      const query = await vscode.window.showInputBox({
        prompt: 'Search query',
        placeHolder: 'search your memory...',
      });
      if (!query) return;

      try {
        const results = await apiRequest<any[]>(
          'POST',
          '/api/v1/vector/search',
          { query, top_k: 10 },
        );

        const items = results.map((r: any) => ({
          label: r.id,
          description: `Score: ${r.score.toFixed(3)}`,
          detail: r.content.substring(0, 200),
        }));

        const picked = await vscode.window.showQuickPick(items, {
          placeHolder: `Found ${results.length} results`,
        });

        if (picked) {
          const doc = await vscode.workspace.openTextDocument({
            content: picked.detail || '',
            language: 'markdown',
          });
          vscode.window.showTextDocument(doc);
        }
      } catch (e: any) {
        vscode.window.showErrorMessage(`Search failed: ${e.message}`);
      }
    },
  );

  // ── Execute SOP Command ─────────────────────
  const execCmd = vscode.commands.registerCommand(
    'sisyphus.executeSop',
    async () => {
      const sopId = await vscode.window.showInputBox({
        prompt: 'SOP ID to execute',
        placeHolder: 'sop-id',
      });
      if (!sopId) return;

      try {
        vscode.window.withProgress(
          {
            location: vscode.ProgressLocation.Notification,
            title: `Executing SOP: ${sopId}`,
            cancellable: false,
          },
          async () => {
            const result = await apiRequest<any>('POST', '/api/v1/execute', {
              sop_id: sopId,
              context: {},
            });
            vscode.window.showInformationMessage(
              `SOP result: ${result.status} (iterations: ${result.iterations})`,
            );
          },
        );
      } catch (e: any) {
        vscode.window.showErrorMessage(`Execution failed: ${e.message}`);
      }
    },
  );

  context.subscriptions.push(statusCmd, newSessionCmd, searchCmd, execCmd);
}

function getWebviewContent(health: HealthResponse, sessionCount: number): string {
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <style>
    body { font-family: var(--vscode-font-family); padding: 20px; }
    h1 { color: var(--vscode-foreground); }
    .stat { margin: 10px 0; padding: 10px; background: var(--vscode-editor-inactiveSelectionBackground); border-radius: 4px; }
    .label { color: var(--vscode-descriptionForeground); }
    .value { color: var(--vscode-foreground); font-weight: bold; }
  </style>
</head>
<body>
  <h1>Sisyphus Cognitive Engine</h1>
  <div class="stat"><span class="label">Status:</span> <span class="value">${health.status}</span></div>
  <div class="stat"><span class="label">Version:</span> <span class="value">${health.version}</span></div>
  <div class="stat"><span class="label">Uptime:</span> <span class="value">${health.uptime_secs}s</span></div>
  <div class="stat"><span class="label">Sessions:</span> <span class="value">${sessionCount}</span></div>
</body>
</html>`;
}

export function deactivate() {}
