// Sisyphus VS Code Extension — Enhanced with LSP diagnostics, WebView DAG, Agent panel

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

async function apiRequest<T>(method: string, path: string, body?: unknown): Promise<T> {
  const config = getConfig();
  const headers: Record<string, string> = { 'Content-Type': 'application/json' };
  if (config.authToken) headers['Authorization'] = `Bearer ${config.authToken}`;

  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 30000);

  try {
    const resp = await fetch(`${config.baseUrl}${path}`, {
      method, headers,
      body: body ? JSON.stringify(body) : undefined,
      signal: controller.signal,
    });
    if (!resp.ok) throw new Error(`API ${resp.status}: ${await resp.text()}`);
    return (await resp.json()) as T;
  } finally { clearTimeout(timer); }
}

interface HealthResponse { status: string; version: string; uptime_secs: number; }
interface DagNode { id: string; label: string; status: string; x: number; y: number; }
interface DagEdge { from: string; to: string; }
interface DagGraph { nodes: DagNode[]; edges: DagEdge[]; width: number; height: number; }
interface LspDiagnostic { file: string; line: number; column: number; message: string; severity: string; }

// ── LSP Diagnostic Collection ─────────────────
let diagnosticCollection: vscode.DiagnosticCollection;

async function refreshLspDiagnostics() {
  try {
    const diags = await apiRequest<LspDiagnostic[]>('GET', '/api/v1/lsp/diagnostics');
    diagnosticCollection.clear();

    const byFile = new Map<string, vscode.Diagnostic[]>();
    for (const d of diags) {
      if (!byFile.has(d.file)) byFile.set(d.file, []);
      const range = new vscode.Range(
        new vscode.Position(d.line, d.column),
        new vscode.Position(d.line, d.column + 1),
      );
      const severity = d.severity === 'error' ? vscode.DiagnosticSeverity.Error :
                       d.severity === 'warning' ? vscode.DiagnosticSeverity.Warning :
                       vscode.DiagnosticSeverity.Information;
      byFile.get(d.file)!.push(new vscode.Diagnostic(range, d.message, severity));
    }

    for (const [file, diags] of byFile) {
      const uri = vscode.Uri.file(file);
      diagnosticCollection.set(uri, diags);
    }
  } catch { /* bridge offline, ignore */ }
}

// ──
class DagWebviewProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = 'sisyphus.dagView';
  private view?: vscode.WebviewView;

  resolveWebviewView(webviewView: vscode.WebviewView) {
    this.view = webviewView;
    webviewView.webview.options = { enableScripts: true };
    webviewView.webview.html = `<html><body style="background:#0d1117;color:#c9d1d9;font-family:system-ui;padding:16px;">
      <h3 style="color:#58a6ff">DAG Visualization</h3>
      <p>Select a SOP to view its execution graph.</p>
      <button onclick="fetch('/api/sops/default/dag').then(r=>r.json()).then(g=>render(g))" style="background:#238636;color:#fff;border:none;padding:6px 12px;border-radius:4px;cursor:pointer;">Load DAG</button>
      <div id="dag" style="margin-top:12px;"></div>
    </body></html>`;
  }

  async showDag(sopId: string) {
    if (!this.view) return;
    try {
      const graph = await apiRequest<DagGraph>('GET', `/api/sops/${sopId}/dag`);
      this.view.webview.html = this.renderDagHtml(graph);
    } catch (e: any) {
      this.view.webview.html = `<html><body style="background:#0d1117;color:#c9d1d9;font-family:system-ui;padding:16px;">
        <p style="color:#f85149">Error loading DAG: ${e.message}</p></body></html>`;
    }
  }

  private renderDagHtml(graph: DagGraph): string {
    if (graph.nodes.length === 0) return '<html><body style="background:#0d1117;color:#8b949e;padding:16px;"><p>No DAG to display.</p></body></html>';

    const w = graph.width + 40; const h = graph.height + 40;
    let svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" style="width:100%;height:auto;">`;
    svg += `<defs><marker id="arrow" markerWidth="10" markerHeight="7" refX="10" refY="3.5" orient="auto"><polygon points="0 0,10 3.5,0 7" fill="#30363d"/></marker></defs>`;

    for (const edge of graph.edges) {
      const f = graph.nodes.find(n => n.id === edge.from);
      const t = graph.nodes.find(n => n.id === edge.to);
      if (f && t) svg += `<line x1="${f.x+60}" y1="${f.y+20}" x2="${t.x+60}" y2="${t.y+20}" stroke="#30363d" stroke-width="2" marker-end="url(#arrow)"/>`;
    }

    for (const node of graph.nodes) {
      const color = node.status === 'completed' ? '#3fb950' : node.status === 'running' ? '#d29922' : node.status === 'failed' ? '#f85149' : '#484f58';
      svg += `<rect x="${node.x}" y="${node.y}" width="120" height="40" rx="6" fill="#161b22" stroke="${color}" stroke-width="2"/>`;
      svg += `<text x="${node.x+60}" y="${node.y+25}" text-anchor="middle" fill="#c9d1d9" font-size="12" font-family="system-ui">${node.label}</text>`;
    }

    svg += '</svg>';
    return `<html><body style="background:#0d1117;color:#c9d1d9;font-family:system-ui;padding:16px;">
      <h3 style="color:#58a6ff">DAG Visualization</h3>${svg}</body></html>`;
  }
}
// ── Agent Panel ────────────────────────────────
class AgentPanelProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = 'sisyphus.agentView';
  private view?: vscode.WebviewView;

  resolveWebviewView(webviewView: vscode.WebviewView) {
    this.view = webviewView;
    webviewView.webview.options = { enableScripts: true };
    webviewView.webview.html = `<html><body style="background:#0d1117;color:#c9d1d9;font-family:system-ui;padding:16px;">
      <h3 style="color:#58a6ff">Agent</h3>
      <div id="status" style="margin:8px 0;padding:8px;background:#161b22;border-radius:4px;">Loading...</div>
      <textarea id="input" style="width:100%;height:60px;background:#0d1117;border:1px solid #30363d;color:#c9d1d9;border-radius:4px;padding:8px;font-family:monospace;" placeholder="Send message to agent..."></textarea>
      <button onclick="send()" style="margin-top:8px;background:#238636;color:#fff;border:none;padding:6px 12px;border-radius:4px;cursor:pointer;">Send</button>
      <div id="response" style="margin-top:12px;padding:8px;background:#161b22;border-radius:4px;white-space:pre-wrap;"></div>
    </body></html>`;
  }
}

// ── Extension Activation ──────────────────────
export function activate(context: vscode.ExtensionContext) {
  diagnosticCollection = vscode.languages.createDiagnosticCollection('sisyphus');

  // Status bar
  const statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusItem.text = '$(sync~spin) Sisyphus';
  statusItem.tooltip = 'Checking Sisyphus...';
  statusItem.show();
  context.subscriptions.push(statusItem);

  // Update status periodically
  const updateStatus = async () => {
    try {
      const h = await apiRequest<HealthResponse>('GET', '/health');
      statusItem.text = `$(check) Sisyphus v${h.version}`;
      statusItem.tooltip = `Status: ${h.status} | Uptime: ${h.uptime_secs}s`;
    } catch {
      statusItem.text = '$(warning) Sisyphus offline';
      statusItem.tooltip = 'Cannot connect to Sisyphus bridge';
    }
  };
  updateStatus();
  setInterval(updateStatus, 15000);

  // LSP diagnostics refresh
  refreshLspDiagnostics();
  setInterval(refreshLspDiagnostics, 30000);

  // Commands
  context.subscriptions.push(
    vscode.commands.registerCommand('sisyphus.status', async () => {
      try {
        const h = await apiRequest<HealthResponse>('GET', '/health');
        const sessions = await apiRequest<any[]>('GET', '/api/sessions');
        const panel = vscode.window.createWebviewPanel('sisyphusStatus', 'Sisyphus Status', vscode.ViewColumn.One, {});
        panel.webview.html = `<html><body style="background:#0d1117;color:#c9d1d9;font-family:system-ui;padding:24px;">
          <h1 style="color:#58a6ff">&#9889; Sisyphus v${h.version}</h1>
          <p>Status: <strong style="color:#3fb950">${h.status}</strong></p>
          <p>Uptime: ${h.uptime_secs}s</p>
          <p>Sessions: ${sessions.length}</p>
        </body></html>`;
      } catch (e: any) { vscode.window.showErrorMessage(`Sisyphus offline: ${e.message}`); }
    }),

    vscode.commands.registerCommand('sisyphus.newSession', async () => {
      const name = await vscode.window.showInputBox({ prompt: 'Session name', placeHolder: 'my-session' });
      if (!name) return;
      try {
        const s = await apiRequest<any>('POST', '/api/sessions', { name });
        vscode.window.showInformationMessage(`Session created: ${s.id}`);
      } catch (e: any) { vscode.window.showErrorMessage(`Failed: ${e.message}`); }
    }),

    vscode.commands.registerCommand('sisyphus.searchMemory', async () => {
      const query = await vscode.window.showInputBox({ prompt: 'Search memory', placeHolder: 'search...' });
      if (!query) return;
      try {
        const results = await apiRequest<any[]>('POST', '/api/search', { query, top_k: 10 });
        const items = results.map((r: any) => ({ label: r.id, description: `Score: ${r.score.toFixed(3)}`, detail: r.content.substring(0, 200) }));
        const picked = await vscode.window.showQuickPick(items, { placeHolder: `${results.length} results` });
        if (picked) {
          const doc = await vscode.workspace.openTextDocument({ content: picked.detail || '', language: 'markdown' });
          vscode.window.showTextDocument(doc);
        }
      } catch (e: any) { vscode.window.showErrorMessage(`Search failed: ${e.message}`); }
    }),

    vscode.commands.registerCommand('sisyphus.executeSop', async () => {
      const sopId = await vscode.window.showInputBox({ prompt: 'SOP ID', placeHolder: 'sop-id' });
      if (!sopId) return;
      vscode.window.withProgress({ location: vscode.ProgressLocation.Notification, title: `Executing: ${sopId}` }, async () => {
        const r = await apiRequest<any>('POST', '/api/execute', { sop_id: sopId, context: {} });
        vscode.window.showInformationMessage(`Result: ${r.status} (${r.iterations} iterations)`);
      });
    }),

    vscode.commands.registerCommand('sisyphus.showDag', async () => {
      const sopId = await vscode.window.showInputBox({ prompt: 'SOP ID for DAG', placeHolder: 'sop-id' });
      if (sopId) {
        const panel = vscode.window.createWebviewPanel('sisyphusDag', `DAG: ${sopId}`, vscode.ViewColumn.One, { enableScripts: true });
        try {
          const graph = await apiRequest<DagGraph>('GET', `/api/sops/${sopId}/dag`);
          // Render SVG in webview
          let svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${graph.width+40} ${graph.height+40}" style="width:100%;background:#0d1117;">`;
          for (const node of graph.nodes) {
            const c = node.status==='completed'?'#3fb950':node.status==='failed'?'#f85149':'#484f58';
            svg += `<rect x="${node.x}" y="${node.y}" width="120" height="40" rx="6" fill="#161b22" stroke="${c}" stroke-width="2"/>`;
            svg += `<text x="${node.x+60}" y="${node.y+25}" text-anchor="middle" fill="#c9d1d9" font-size="12">${node.label}</text>`;
          }
          svg += '</svg>';
          panel.webview.html = `<html><body style="background:#0d1117;padding:16px;">${svg}</body></html>`;
        } catch (e: any) { panel.webview.html = `<html><body style="color:#f85149;padding:16px;">Error: ${e.message}</body></html>`; }
      }
    }),
  );

  // Register webview providers
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(DagWebviewProvider.viewType, new DagWebviewProvider()),
    vscode.window.registerWebviewViewProvider(AgentPanelProvider.viewType, new AgentPanelProvider()),
  );
}

export function deactivate() {}
