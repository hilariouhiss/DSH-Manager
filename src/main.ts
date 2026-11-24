import "@fontsource/ibm-plex-sans/400.css";
import "@fontsource/ibm-plex-sans/500.css";
import "@fontsource/ibm-plex-sans/600.css";
import "@fontsource/ibm-plex-mono/400.css";
import "@fontsource/ibm-plex-mono/500.css";
import { invoke } from "@tauri-apps/api/core";

type Status = { state: string; managed: boolean; pid: number | null; url: string | null; port: number };
type Versions = { tags: Record<string, string>; versions: string[] };
type EnvInfo = {
  node: string | null;
  node_version: string | null;
  npm: string | null;
  npm_root: string | null;
  registry: string | null;
  dsh_home: string | null;
};
type Installed = { version: string; path: string };
type LogTail = { text: string; active: boolean };

const el = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const ESCAPES: Record<string, string> = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const esc = (value: string) => value.replace(/[&<>"']/g, (char) => ESCAPES[char] ?? char);

const VISIBLE_VERSIONS = 5;

let port = Number(localStorage.getItem("port") ?? "3080") || 3080;
let openBrowser = localStorage.getItem("openBrowser") !== "0";
let status: Status | null = null;
let installed: Installed | null = null;
let versions: Versions | null = null;
let busy: string | null = null;
let showAll = false;
let logKind: "web" | "npm" = "web";

function setBanner(message: string | null) {
  const node = el<HTMLDivElement>("banner");
  if (message) {
    node.textContent = message;
    node.classList.remove("hidden");
  } else {
    node.textContent = "";
    node.classList.add("hidden");
  }
}

function renderEnv(info: EnvInfo) {
  const scope = [
    info.node_version ? "Node " + info.node_version : "Node 缺失",
    info.npm ? "npm 可用" : "npm 缺失",
    info.registry ? info.registry.replace(/^https?:\/\//, "").replace(/\/$/, "") : "",
  ].filter((part) => part.length > 0);
  el("env-scope").textContent = scope.join(" · ");
  const detail = [
    info.npm_root ? "npm root " + info.npm_root : "",
    info.dsh_home ? "DSH_HOME " + info.dsh_home : "",
  ].filter((part) => part.length > 0);
  const detailNode = el("env-detail");
  detailNode.textContent = detail.join("    ");
  detailNode.title = detail.join("\n");
  if (!info.node || !info.npm) {
    setBanner("未找到 node 或 npm。请先安装 Node.js（DSH 0.2.3+ 需要 24.22.3+ 或 26.13.1+）。");
  }
}

function renderInstalled() {
  el("installed-version").textContent = installed ? "已安装 " + installed.version : "未安装";
}

function renderStatus() {
  const running = status?.state === "running";
  const statusValue = el("status-value");
  statusValue.textContent = running ? "运行中" : "已停止";
  statusValue.className = "cell-value lead " + (running ? "accent" : "muted");
  el("port-cell").textContent = String(status?.port ?? port);
  el("pid-cell").textContent = status?.pid ? String(status.pid) : "—";
  const manageCell = el("manage-cell");
  manageCell.textContent = running ? (status?.managed ? "托管" : "外部") : "—";
  manageCell.className = "cell-value " + (running ? "" : "muted");
  el("finding").textContent = running
    ? (status?.url ?? "") + " · PID " + (status?.pid ?? "—") + " · " + (status?.managed ? "本应用托管" : "非本应用启动")
    : "端口 " + port + " 空闲 · 已安装 " + (installed?.version ?? "—");

  const primary = el<HTMLButtonElement>("btn-primary");
  primary.textContent = running ? "停止" : "启动";
  primary.disabled = busy !== null;
  el<HTMLButtonElement>("btn-restart").disabled = busy !== null || !running;
  el<HTMLButtonElement>("btn-open").disabled = busy !== null || !running;

  const busyNode = el("busy");
  busyNode.textContent = busy ?? "";
  busyNode.classList.toggle("hidden", busy === null);
}

function tagOf(version: string): string {
  if (!versions) return "";
  return Object.entries(versions.tags)
    .filter(([, tagged]) => tagged === version)
    .map(([name]) => name)
    .join(" / ");
}

function renderTags() {
  const host = el("tags");
  if (!versions) {
    host.innerHTML = "";
    return;
  }
  const entries = Object.entries(versions.tags);
  if (entries.length === 0) {
    host.innerHTML = '<span class="empty">registry 未返回通道标签</span>';
    return;
  }
  host.innerHTML = entries
    .map(([name, version]) => {
      const active = installed?.version === version;
      return (
        '<button type="button" class="channel' + (active ? " active" : "") + '" data-install="' + esc(version) + '">' +
        esc(name) +
        '<span class="ch-version">' + esc(version) + "</span>" +
        "</button>"
      );
    })
    .join("");
}

function renderVersions() {
  const host = el("version-list");
  if (!versions) {
    host.innerHTML = '<p class="empty">正在读取版本列表…</p>';
    return;
  }
  const list = versions.versions.slice().reverse();
  const shown = showAll ? list : list.slice(0, VISIBLE_VERSIONS);
  const current = installed?.version ?? null;
  host.innerHTML = shown
    .map((version) => {
      const isCurrent = version === current;
      const tag = tagOf(version);
      return (
        '<div class="ver' + (isCurrent ? " current" : "") + '">' +
        '<span class="ver-name">' + esc(version) +
        (tag ? ' <span class="ver-tag">' + esc(tag) + "</span>" : "") +
        "</span>" +
        (isCurrent
          ? '<span class="ver-state">已安装</span>'
          : '<button type="button" class="btn small" data-install="' + esc(version) + '">安装</button>') +
        "</div>"
      );
    })
    .join("");
  const showAllButton = el<HTMLButtonElement>("btn-show-all");
  showAllButton.classList.toggle("hidden", versions.versions.length <= VISIBLE_VERSIONS);
  showAllButton.textContent = showAll ? "收起版本列表" : "显示全部版本（" + versions.versions.length + "）";
}

function renderFresh() {
  const time = new Date().toLocaleTimeString("zh-CN", { hour12: false });
  el("fresh").textContent = "检测于 " + time;
}

async function loadEnv() {
  try {
    renderEnv(await invoke<EnvInfo>("env_info"));
  } catch (error) {
    setBanner(String(error));
  }
}

async function loadInstalled() {
  try {
    installed = await invoke<Installed | null>("installed");
    renderInstalled();
    renderTags();
    renderVersions();
  } catch (error) {
    setBanner(String(error));
  }
}

async function loadVersions(force = false) {
  try {
    versions = await invoke<Versions>("versions", { force });
    renderTags();
    renderVersions();
    renderStatus();
  } catch (error) {
    setBanner("版本查询失败：" + String(error));
  }
}

async function refreshStatus() {
  try {
    status = await invoke<Status>("status", { port });
    renderStatus();
    renderFresh();
  } catch {
    // 轮询失败不打扰用户，下一次再看
  }
}

async function refreshLog() {
  try {
    const tail = await invoke<LogTail>("read_log", { kind: logKind });
    const node = el("log");
    const text = tail.text.trim().length > 0 ? tail.text : "（暂无输出）";
    if (node.textContent !== text) {
      const atBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 40;
      node.textContent = text;
      if (atBottom) node.scrollTop = node.scrollHeight;
    }
  } catch {
    // 日志读取失败忽略
  }
}

async function act(label: string, work: () => Promise<void>) {
  if (busy !== null) return;
  busy = label;
  setBanner(null);
  renderStatus();
  try {
    await work();
  } catch (error) {
    setBanner(String(error));
  } finally {
    busy = null;
    await refreshStatus();
    renderStatus();
    void refreshLog();
  }
}

async function startDsh() {
  await act("正在启动 DSH（首次启动可能要一分钟）…", async () => {
    status = await invoke<Status>("start", { port, openBrowser });
    renderStatus();
  });
}

async function stopDsh() {
  if (status?.state !== "running") return;
  const hint = status.managed
    ? "停止 DSH？该实例上正在运行的会话会被中断。"
    : "该 DSH 不是本应用启动的（端口 " + port + "）。停止会中断其上的所有会话，确定继续？";
  if (!confirm(hint)) return;
  await act("正在停止 DSH…", async () => {
    status = await invoke<Status>("stop", { port });
    renderStatus();
  });
}

async function restartDsh() {
  if (!confirm("重启 DSH？当前运行中的会话会被中断。")) return;
  await act("正在重启 DSH…", async () => {
    await invoke("stop", { port });
    await new Promise((resolve) => setTimeout(resolve, 800));
    status = await invoke<Status>("start", { port, openBrowser });
    renderStatus();
  });
}

async function installVersion(version: string) {
  if (!version || busy !== null) return;
  const message =
    status?.state === "running"
      ? "安装 " + version + " 前需要先停止运行中的 DSH（会中断其上的会话）。继续？"
      : "安装 @deepseek-ai/dsh@" + version + "？";
  if (!confirm(message)) return;
  await act("正在安装 " + version + "，日志见下方 npm 标签页…", async () => {
    const result = await invoke<Installed | null>("install", {
      version,
      restart: el<HTMLInputElement>("restart-after").checked,
      port,
      openBrowser,
    });
    if (result) installed = result;
    renderInstalled();
    await loadVersions(true);
  });
}

function wire() {
  const portInput = el<HTMLInputElement>("port");
  portInput.value = String(port);
  el<HTMLInputElement>("open-browser").checked = openBrowser;
  el<HTMLInputElement>("restart-after").checked = localStorage.getItem("restartAfter") !== "0";

  portInput.addEventListener("change", () => {
    const value = Number(portInput.value);
    if (Number.isFinite(value) && value > 0 && value < 65536) {
      port = Math.floor(value);
      portInput.value = String(port);
      localStorage.setItem("port", String(port));
    } else {
      portInput.value = String(port);
    }
    void refreshStatus();
  });

  el("open-browser").addEventListener("change", () => {
    openBrowser = el<HTMLInputElement>("open-browser").checked;
    localStorage.setItem("openBrowser", openBrowser ? "1" : "0");
  });

  el("restart-after").addEventListener("change", () => {
    const checked = el<HTMLInputElement>("restart-after").checked;
    localStorage.setItem("restartAfter", checked ? "1" : "0");
  });

  el("btn-primary").addEventListener("click", () => {
    if (status?.state === "running") void stopDsh();
    else void startDsh();
  });
  el("btn-restart").addEventListener("click", () => void restartDsh());
  el("btn-open").addEventListener("click", () => {
    const url = status?.url;
    if (!url) return;
    invoke("open_url", { url }).catch((error) => setBanner(String(error)));
  });
  el("btn-check").addEventListener("click", () => {
    void act("正在查询 npm registry…", async () => {
      await loadVersions(true);
    });
  });
  el("btn-show-all").addEventListener("click", () => {
    showAll = !showAll;
    renderVersions();
  });

  document.body.addEventListener("click", (event) => {
    const target = event.target as HTMLElement | null;
    const installButton = target?.closest<HTMLElement>("[data-install]");
    if (installButton?.dataset.install) {
      void installVersion(installButton.dataset.install);
    }
  });

  document.querySelectorAll<HTMLElement>(".tab").forEach((tab) => {
    tab.addEventListener("click", () => {
      logKind = tab.dataset.kind === "npm" ? "npm" : "web";
      document
        .querySelectorAll<HTMLElement>(".tab")
        .forEach((other) => other.classList.toggle("active", other === tab));
      el("log").textContent = "（读取中…）";
      void refreshLog();
    });
  });
}

window.addEventListener("DOMContentLoaded", () => {
  wire();
  renderInstalled();
  renderStatus();
  void refreshStatus();
  void refreshLog();
  void loadEnv();
  void loadInstalled().then(() => loadVersions());
  window.setInterval(() => void refreshStatus(), 2000);
  window.setInterval(() => void refreshLog(), 1500);
});
