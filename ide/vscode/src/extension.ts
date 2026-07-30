// Sisyphus VS Code Extension
// Enhanced with: StatusBar connection state, Session TreeView, Agent panel, config change listener

import * as vscode from 'vscode';

// ── Types ──────────────────────────────────────────

interface SisyphusConfig {
  baseUrl: string;
  authToken: string;
  refreshInterval: number;
  autoRefreshSessions: boolean;
}

interface HealthResponse {
  status: string;
  version: string;
  uptime_secs: number;
}

interface SessionInfo {
  id: string;
  name: string;
  status: string;
  created_at: string;
  updated_at: string;
}

interface DagNode {
  id: string;
  label: string;
  status: string;
  x: number;
  y: number;
}
interface DagEdge {
  from: string;
  to: string;
}
interface DagGraph {
  nodes: DagNode[];
  edges: DagEdge[];
  width: number;
  height: number;
}

interface AgentInfo {
  id: string;
  name: string;
  role: string;
  status: string;
  task: string;
}

interface LspDiagnostic {
  file: string;
  line: number;
  column: number;
  message: string;
  severity: string;
}

// ── Configuration ──────────────────────────────────

function getConfig(): SisyphusConfig {
  const cfg = vscode.workspace.getConfiguration('sisyphus');
  return {
    baseUrl: cfg.get<string>('baseUrl', 'http://localhost:8080'),
    authToken: cfg.get<string>('authToken', ''),
    refreshInterval: cfg.get<number>('refreshInterval', 15),
    autoRefreshSessions: cfg.get<boolean>('autoRefreshSessions', true),
  };
}

// ── API Client ─────────────────────────────────────

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

// ── LSP Diagnostic Collection ──────────────────────

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

// ── Status Bar ─────────────────────────────────────

function createStatusBarItem(context: vscode.ExtensionContext): vscode.StatusBarItem {
  const statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusItem.text = '$(sync~spin) Sisyphus';
  statusItem.tooltip = 'Checking Sisyphus connection...';
  statusItem.backgroundColor = undefined; // default transparent
  statusItem.show();
  context.subscriptions.push(statusItem);

  let statusTimer: ReturnType<typeof setInterval> | undefined;

  const updateStatus = async () => {
    try {
      const h = await apiRequest<HealthResponse>('GET', '/health');
      statusItem.text = `$(check) Sisyphus v${h.version}`;
      statusItem.tooltip = `Status: ${h.status} | Uptime: ${h.uptime_secs}s`;
      statusItem.backgroundColor = new vscode.ThemeColor('statusBarItem.prominentBackground');
    } catch {
      statusItem.text = '$(warning) Sisyphus offline';
      statusItem.tooltip = 'Cannot connect to Sisyphus bridge. Check config.sisyphus.baseUrl';
      statusItem.backgroundColor = new vscode.ThemeColor('statusBarItem.errorBackground');
    }
  };

  const startPolling = () => {
    const cfg = getConfig();
    if (statusTimer) clearInterval(statusTimer);
    updateStatus();
    statusTimer = setInterval(updateStatus, cfg.refreshInterval * 1000);
  };

  startPolling();

  // Restart polling when config changes
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('sisyphus')) startPolling();
    }),
  );

  return statusItem;
}

// ── Session TreeView ───────────────────────────────

class SessionTreeItem extends vscode.TreeItem {
  constructor(
    public readonly session: SessionInfo,
  ) {
    super(session.name || session.id, vscode.TreeItemCollapsibleState.None);

    this.id = session.id;
    this.description = session.status;
    this.tooltip = `ID: ${session.id}\nCreated: ${session.created_at}\nUpdated: ${session.updated_at}`;

    // Icon based on session status
    if (session.status === 'active' || session.status === 'running') {
      this.iconPath = new vscode.ThemeIcon('play-circle', new vscode.ThemeColor('terminal.ansiGreen'));
    } else if (session.status === 'completed' || session.status === 'idle') {
      this.iconPath = new vscode.ThemeIcon('check-circle', new vscode.ThemeColor('terminal.ansiBrightBlack'));
    } else if (session.status === 'failed' || session.status === 'error') {
      this.iconPath = new vscode.ThemeIcon('error', new vscode.ThemeColor('terminal.ansiRed'));
    } else {
      this.iconPath = new vscode.ThemeIcon('circle-outline');
    }

    // Context value for conditional menu items
    this.contextValue = 'session';

    // Command when clicked
    this.command = {
      command: 'sisyphus.status',
      title: 'Show Session Detail',
      arguments: [session.id],
    };
  }
}

class SessionTreeDataProvider implements vscode.TreeDataProvider<SessionTreeItem> {
  private _onDidChangeTreeData = new vscode.EventEmitter<SessionTreeItem | undefined | null>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  private sessions: SessionInfo[] = [];
  private refreshTimer: ReturnType<typeof setInterval> | undefined;

  constructor(private context: vscode.ExtensionContext) {
    this.startAutoRefresh();
  }

  getTreeItem(element: SessionTreeItem): vscode.TreeItem {
    return element;
  }

  getChildren(_element?: SessionTreeItem): Thenable<SessionTreeItem[]> {
    if (_element) return Promise.resolve([]); // no children for leaf nodes
    return Promise.resolve(this.sessions.map(s => new SessionTreeItem(s)));
  }

  async refresh(): Promise<void> {
    try {
      this.sessions = await apiRequest<SessionInfo[]>('GET', '/api/sessions');
    } catch {
      this.sessions = [];
    }
    this._onDidChangeTreeData.fire(null);
  }

  private startAutoRefresh(): void {
    const cfg = getConfig();
    if (cfg.autoRefreshSessions) {
      this.refresh();
      this.refreshTimer = setInterval(() => this.refresh(), 15000);
    }
  }

  dispose(): void {
    if (this.refreshTimer) clearInterval(this.refreshTimer);
  }
}

// ── Agent Panel ────────────────────────────────────

class AgentPanelProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = 'sisyphus.agentView';
  private view?: vscode.WebviewView;
  private statusTimer: ReturnType<typeof setInterval> | undefined;

  resolveWebviewView(webviewView: vscode.WebviewView) {
    this.view = webviewView;
    webviewView.webview.options = { enableScripts: true };
    webviewView.webview.html = this.getInitialHtml();

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(async (msg) => {
      switch (msg.type) {
        case 'send':
          try {
            const result = await apiRequest<any>('POST', '/api/agent/chat', { message: msg.text });
            webviewView.webview.postMessage({ type: 'response', text: JSON.stringify(result, null, 2) });
          } catch (e: any) {
            webviewView.webview.postMessage({ type: 'response', text: `Error: ${e.message}` });
          }
          break;
        case 'refreshAgents':
          try {
            const agents = await apiRequest<AgentInfo[]>('GET', '/api/agents');
            webviewView.webview.postMessage({ type: 'agents', agents });
          } catch { /* ignore */ }
          break;
      }
    });

    // Refresh agent status periodically
    this.statusTimer = setInterval(() => {
      this.refreshAgentStatus();
    }, 10000);
  }

  private async refreshAgentStatus() {
    if (!this.view) return;
    try {
      const agents = await apiRequest<AgentInfo[]>('GET', '/api/agents');
      this.view.webview.postMessage({ type: 'agents', agents });
    } catch { /* ignore */ }
  }

  private getInitialHtml(): string {
    return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }
  body { background:#0d1117; color:#c9d1d9; font-family: system-ui, -apple-system, sans-serif; padding:16px; font-size:13px; }
  h3 { color:#58a6ff; margin-bottom:12px; font-size:14px; }
  .card { background:#161b22; border:1px solid #30363d; border-radius:6px; padding:10px; margin-bottom:10px; }
  .agent-row { display:flex; align-items:center; gap:8px; padding:6px 0; border-bottom:1px solid #21262d; }
  .agent-row:last-child { border-bottom:none; }
  .agent-status { width:8px; height:8px; border-radius:50%; display:inline-block; flex-shrink:0; }
  .status-idle { background:#484f58; }
  .status-running { background:#d29922; }
  .status-completed { background:#3fb950; }
  .status-failed { background:#f85149; }
  .agent-name { flex:1; color:#e6edf3; }
  .agent-role { color:#8b949e; font-size:11px; }
  textarea { width:100%; height:60px; background:#0d1117; border:1px solid #30363d; color:#c9d1d9; border-radius:6px; padding:8px; font-family: 'Cascadia Code', 'Fira Code', monospace; font-size:12px; resize:vertical; }
  textarea:focus { outline:none; border-color:#58a6ff; }
  button { background:#238636; color:#fff; border:none; padding:6px 14px; border-radius:6px; cursor:pointer; font-size:12px; font-weight:500; }
  button:hover { background:#2ea043; }
  button.secondary { background:#21262d; color:#c9d1d9; border:1px solid #30363d; }
  button.secondary:hover { background:#30363d; }
  .response { margin-top:10px; padding:10px; background:#161b22; border:1px solid #30363d; border-radius:6px; white-space:pre-wrap; font-family: 'Cascadia Code', 'Fira Code', monospace; font-size:11px; max-height:200px; overflow-y:auto; }
  .toolbar { display:flex; gap:6px; margin-top:8px; }
  .empty { color:#8b949e; text-align:center; padding:16px 0; font-style:italic; }
</style>
</head>
<body>
  <h3>$(organization) Agents</h3>
  <div class="card">
    <div id="agent-list"><div class="empty">Loading agents...</div></div>
  </div>

  <h3>$(comment) Chat</h3>
  <textarea id="input" placeholder="Send message to agent..."></textarea>
  <div class="toolbar">
    <button onclick="send()">Send</button>
  </div>
  <div id="response" class="response">Ready.</div>

<script>
  const vscode = acquireVsCodeApi();

  // Listen for messages from the extension host
  window.addEventListener('message', event => {
    const msg = event.data;
    if (msg.type === 'response') {
      document.getElementById('response').textContent = msg.text;
    } else if (msg.type === 'agents') {
      renderAgents(msg.agents);
    }
  });

  function send() {
    const input = document.getElementById('input');
    const text = input.value.trim();
    if (!text) return;
    document.getElementById('response').textContent = 'Sending...';
    vscode.postMessage({ type: 'send', text });
    input.value = '';
  }

  function renderAgents(agents) {
    const container = document.getElementById('agent-list');
    if (!agents || agents.length === 0) {
      container.innerHTML = '<div class="empty">No agents available.</div>';
      return;
    }
    container.innerHTML = agents.map(a => {
      const statusClass = 'status-' + (a.status || 'idle');
      return '<div class="agent-row">' +
        '<span class="agent-status ' + statusClass + '"></span>' +
        '<span class="agent-name">' + esc(a.name || a.id) + '</span>' +
        '<span class="agent-role">' + esc(a.role || '') + '</span>' +
        '</div>';
    }).join('');
  }

  function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }

  // Initial agent load
  vscode.postMessage({ type: 'refreshAgents' });

  // Send on Ctrl+Enter
  document.getElementById('input').addEventListener('keydown', e => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) send();
  });
</script>
</body>
</html>`;
  }

  dispose(): void {
    if (this.statusTimer) clearInterval(this.statusTimer);
  }
}

// ── DAG Webview Provider ───────────────────────────

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

// ── Extension Activation ───────────────────────────

export function activate(context: vscode.ExtensionContext) {
  diagnosticCollection = vscode.languages.createDiagnosticCollection('sisyphus');
  context.subscriptions.push(diagnosticCollection);

  // Status bar with connection state
  createStatusBarItem(context);

  // LSP diagnostics refresh
  refreshLspDiagnostics();
  const lspRefreshTimer = setInterval(refreshLspDiagnostics, 30000);
  context.subscriptions.push({ dispose: () => clearInterval(lspRefreshTimer) });

  // Session TreeDataProvider
  const sessionProvider = new SessionTreeDataProvider(context);
  const sessionTreeView = vscode.window.createTreeView('sisyphus.sessionView', {
    treeDataProvider: sessionProvider,
    showCollapseAll: false,
  });
  context.subscriptions.push(sessionTreeView, sessionProvider);

  // Commands
  context.subscriptions.push(
    vscode.commands.registerCommand('sisyphus.status', async () => {
      try {
        const h = await apiRequest<HealthResponse>('GET', '/health');
        const sessions = await apiRequest<SessionInfo[]>('GET', '/api/sessions');
        const agents = await apiRequest<AgentInfo[]>('GET', '/api/agents');
        const panel = vscode.window.createWebviewPanel(
          'sisyphusStatus', 'Sisyphus Status', vscode.ViewColumn.One, {}
        );
        const agentCount = Array.isArray(agents) ? agents.length : 0;
        panel.webview.html = `<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><title>Sisyphus Status</title>
<style>
  body { background:#0d1117; color:#c9d1d9; font-family:system-ui; padding:24px; }
  h1 { color:#58a6ff; } .stat { margin:8px 0; } .value { color:#3fb950; font-weight:600; }
  table { width:100%; border-collapse:collapse; margin-top:12px; }
  th, td { text-align:left; padding:6px 8px; border-bottom:1px solid #21262d; }
  th { color:#8b949e; font-size:12px; text-transform:uppercase; }
</style>
</head>
<body>
  <h1>&#9889; Sisyphus v${h.version}</h1>
  <div class="stat">Status: <span class="value">${h.status}</span></div>
  <div class="stat">Uptime: <span class="value">${h.uptime_secs}s</span></div>
  <div class="stat">Sessions: <span class="value">${sessions.length}</span></div>
  <div class="stat">Agents: <span class="value">${agentCount}</span></div>
</body></html>`;
      } catch (e: any) {
        vscode.window.showErrorMessage(`Sisyphus offline: ${e.message}`);
      }
    }),

    vscode.commands.registerCommand('sisyphus.newSession', async () => {
      const name = await vscode.window.showInputBox({
        prompt: 'Session name',
        placeHolder: 'my-session',
        validateInput: (val) => val.trim() ? null : 'Name cannot be empty',
      });
      if (!name) return;
      try {
        const s = await apiRequest<any>('POST', '/api/sessions', { name });
        vscode.window.showInformationMessage(`Session created: ${s.id}`);
        sessionProvider.refresh();
      } catch (e: any) { vscode.window.showErrorMessage(`Failed: ${e.message}`); }
    }),

    vscode.commands.registerCommand('sisyphus.searchMemory', async () => {
      const query = await vscode.window.showInputBox({
        prompt: 'Search memory',
        placeHolder: 'search query...',
      });
      if (!query) return;
      try {
        const results = await apiRequest<any[]>('POST', '/api/search', { query, top_k: 10 });
        const items = results.map((r: any) => ({
          label: r.id,
          description: `Score: ${r.score.toFixed(3)}`,
          detail: (r.content || '').substring(0, 200),
        }));
        const picked = await vscode.window.showQuickPick(items, {
          placeHolder: `${results.length} results`,
        });
        if (picked) {
          const doc = await vscode.workspace.openTextDocument({
            content: picked.detail || '',
            language: 'markdown',
          });
          vscode.window.showTextDocument(doc);
        }
      } catch (e: any) { vscode.window.showErrorMessage(`Search failed: ${e.message}`); }
    }),

    vscode.commands.registerCommand('sisyphus.executeSop', async () => {
      const sopId = await vscode.window.showInputBox({
        prompt: 'SOP ID',
        placeHolder: 'sop-id',
      });
      if (!sopId) return;
      await vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: `Executing: ${sopId}` },
        async () => {
          try {
            const r = await apiRequest<any>('POST', '/api/execute', { sop_id: sopId, context: {} });
            vscode.window.showInformationMessage(`Result: ${r.status} (${r.iterations} iterations)`);
          } catch (e: any) { vscode.window.showErrorMessage(`Execution failed: ${e.message}`); }
        },
      );
    }),

    vscode.commands.registerCommand('sisyphus.showDag', async () => {
      const sopId = await vscode.window.showInputBox({
        prompt: 'SOP ID for DAG',
        placeHolder: 'sop-id',
      });
      if (sopId) {
        const panel = vscode.window.createWebviewPanel(
          'sisyphusDag', `DAG: ${sopId}`, vscode.ViewColumn.One, { enableScripts: true }
        );
        try {
          const graph = await apiRequest<DagGraph>('GET', `/api/sops/${sopId}/dag`);
          let svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${graph.width+40} ${graph.height+40}" style="width:100%;background:#0d1117;">`;
          for (const node of graph.nodes) {
            const c = node.status==='completed'?'#3fb950':node.status==='failed'?'#f85149':'#484f58';
            svg += `<rect x="${node.x}" y="${node.y}" width="120" height="40" rx="6" fill="#161b22" stroke="${c}" stroke-width="2"/>`;
            svg += `<text x="${node.x+60}" y="${node.y+25}" text-anchor="middle" fill="#c9d1d9" font-size="12">${node.label}</text>`;
          }
          svg += '</svg>';
          panel.webview.html = `<html><body style="background:#0d1117;padding:16px;">${svg}</body></html>`;
        } catch (e: any) {
          panel.webview.html = `<html><body style="color:#f85149;padding:16px;">Error: ${e.message}</body></html>`;
        }
      }
    }),

    vscode.commands.registerCommand('sisyphus.refreshSessions', () => {
      sessionProvider.refresh();
    }),

    vscode.commands.registerCommand('sisyphus.deleteSession', async (sessionItem?: SessionTreeItem) => {
      let sessionId: string | undefined;
      if (sessionItem && sessionItem.session) {
        sessionId = sessionItem.session.id;
      } else {
        sessionId = await vscode.window.showInputBox({
          prompt: 'Session ID to delete',
          placeHolder: 'session-id',
        });
      }
      if (!sessionId) return;
      const confirm = await vscode.window.showWarningMessage(
        `Delete session ${sessionId}?`, { modal: true },
        'Delete',
      );
      if (!confirm) return;
      try {
        await apiRequest<any>('DELETE', `/api/sessions/${sessionId}`);
        vscode.window.showInformationMessage(`Session deleted: ${sessionId}`);
        sessionProvider.refresh();
      } catch (e: any) { vscode.window.showErrorMessage(`Delete failed: ${e.message}`); }
    }),
  );

  // Config change listener
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('sisyphus')) {
        // Re-trigger status update with new config
        // (status bar listener handles this via createStatusBarItem's onDidChangeConfiguration)
        // Refresh sessions if auto-refresh config changed
        if (e.affectsConfiguration('sisyphus.autoRefreshSessions')) {
          const cfg = getConfig();
          if (cfg.autoRefreshSessions) {
            sessionProvider.refresh();
          }
        }
      }
    }),
  );

  // Register webview providers
  const dagProvider = new DagWebviewProvider();
  const agentProvider = new AgentPanelProvider();
  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(DagWebviewProvider.viewType, dagProvider),
    vscode.window.registerWebviewViewProvider(AgentPanelProvider.viewType, agentProvider),
  );

  // Initial session load
  sessionProvider.refresh();
}

export function deactivate() {}
