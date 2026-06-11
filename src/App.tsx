import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  Activity, RefreshCw, Plus, Heart, Zap, Check,
  TrendingUp, BarChart3, AlertTriangle,
  RotateCcw, X, CircleCheck, HelpCircle,
  Settings, Pencil, Bell, Clock, Trash2, ChevronDown,
} from "lucide-react";

// ─── Types ──────────────────────────────────────────────────

interface HeaderPair { key: string; value: string }
interface ServiceConfig {
  id: string; name: string; url: string; method: string;
  interval_secs: number; timeout_secs: number; expected_status: number;
  headers: HeaderPair[]; body: string | null;
}
interface CheckResult {
  service_id: string; name: string; url: string; status: number;
  healthy: boolean; response_time_ms: number; error: string | null; checked_at: string;
}
interface ServiceStatus {
  service_id: string; name: string; url: string;
  last_check: CheckResult | null; uptime_pct: number;
  avg_response_ms: number; total_checks: number; failed_checks: number;
}
interface CheckRecord {
  service_id: string; healthy: boolean; response_time_ms: number;
  status: number; checked_at: string;
}
interface ServiceAlert {
  type: "down" | "recovered"; service_id: string;
  name: string; error?: string | null; checked_at: string;
}
interface AppSettings {
  default_interval_secs: number;
  notifications_enabled: boolean;
  auto_start: boolean;
  history_days: number;
  start_minimized: boolean;
}

// ─── Utilities ──────────────────────────────────────────────

function formatMs(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function getErrorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}

// ─── Focus Trap Hook ────────────────────────────────────────

function useFocusTrap(active: boolean) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!active || !ref.current) return;
    const el = ref.current;
    const getFocusable = () =>
      el.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
    const first = getFocusable()[0];
    first?.focus();
    const handler = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const items = getFocusable();
      if (items.length === 0) return;
      const f = items[0], l = items[items.length - 1];
      if (e.shiftKey && document.activeElement === f) { e.preventDefault(); l?.focus(); }
      else if (!e.shiftKey && document.activeElement === l) { e.preventDefault(); f?.focus(); }
    };
    el.addEventListener("keydown", handler);
    return () => el.removeEventListener("keydown", handler);
  }, [active]);
  return ref;
}

// ─── Skeleton Loading ───────────────────────────────────────

function LoadingSkeleton() {
  return (
    <>
      <div className="skeleton-card" style={{ animationDelay: "0s" }}>
        <div className="skeleton-line w60" />
        <div className="skeleton-line h28 w40" />
        <div style={{ display: "flex", gap: 8 }}>
          <div className="skeleton-line" style={{ flex: 1, height: 44 }} />
          <div className="skeleton-line" style={{ flex: 1, height: 44 }} />
        </div>
      </div>
      {[0, 1, 2].map(i => (
        <div key={i} className="skeleton-row" style={{ animationDelay: `${0.1 + i * 0.05}s` }}>
          <div className="skeleton-circle" />
          <div className="skeleton-lines">
            <div className="skeleton-line w60" />
            <div className="skeleton-line w40" style={{ height: 8 }} />
          </div>
        </div>
      ))}
    </>
  );
}

// ─── Sparkline ──────────────────────────────────────────────

function Sparkline({ data, healthy }: { data: number[]; healthy?: boolean }) {
  const w = 44, h = 16;
  if (data.length < 2) {
    return (
      <svg width={w} height={h} style={{ opacity: 0.2 }} aria-hidden="true">
        <line x1={0} y1={h / 2} x2={w} y2={h / 2} stroke="currentColor" strokeWidth={1} strokeDasharray="2,2" />
      </svg>
    );
  }
  const min = Math.min(...data), max = Math.max(...data), range = max - min || 1;
  const pts = data.map((v, i) => ({
    x: (i / (data.length - 1)) * w,
    y: 2 + (h - 4) - ((v - min) / range) * (h - 4),
  }));
  const path = pts.map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ");
  const color = healthy !== false ? "#34C759" : "#FF3B30";
  return (
    <svg width={w} height={h} aria-hidden="true">
      <path d={path} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" opacity={0.8} />
      <circle cx={pts[pts.length - 1].x} cy={pts[pts.length - 1].y} r="2" fill={color} />
    </svg>
  );
}

// ─── Trend Chart ────────────────────────────────────────────

const BUCKET_COUNT = 12;
const BUCKET_MS = 5 * 60 * 1000;

function fmtTime(t: number): string {
  const d = new Date(t);
  return `${d.getHours().toString().padStart(2, "0")}:${d.getMinutes().toString().padStart(2, "0")}`;
}

function TrendChart({ history }: { history: CheckRecord[] }) {
  const { buckets, totalChecks } = useMemo(() => {
    const now = Date.now();
    const b: { time: number; total: number; healthy: number }[] = [];
    for (let i = BUCKET_COUNT - 1; i >= 0; i--) {
      const start = now - (i + 1) * BUCKET_MS;
      const end = now - i * BUCKET_MS;
      const items = history.filter(r => {
        const t = new Date(r.checked_at).getTime();
        return t >= start && t < end;
      });
      const h = items.filter(r => r.healthy).length;
      b.push({ time: end, total: items.length, healthy: h });
    }
    return { buckets: b, totalChecks: b.reduce((s, x) => s + x.total, 0) };
  }, [history]);

  const maxTotal = Math.max(...buckets.map(b => b.total), 1);

  return (
    <div className="chart-card" role="img" aria-label={`响应趋势图，共 ${totalChecks} 次检测`}>
      <div className="chart-header">
        <BarChart3 />
        <span className="chart-header-text">响应趋势</span>
        <span className="chart-header-total">{totalChecks} 次检测</span>
      </div>
      {totalChecks === 0 ? (
        <div className="chart-empty">
          <TrendingUp />
          <span className="chart-empty-text">暂无趋势数据</span>
        </div>
      ) : (
        <div className="chart-area">
          {buckets.map((b, i) => {
            const h = b.total > 0 ? Math.max((b.total / maxTotal) * 60, 3) : 3;
            const allOk = b.total > 0 && b.healthy === b.total;
            const noneOk = b.total > 0 && b.healthy === 0;
            const failCount = b.total - b.healthy;
            const tooltip = b.total > 0
              ? `${fmtTime(b.time)} — ${b.total} 次检测${failCount > 0 ? `，${failCount} 次异常` : ""}`
              : `${fmtTime(b.time)} — 无数据`;
            return (
              <div key={i} className="chart-column" title={tooltip}>
                <span className="chart-value">{b.total > 0 ? b.total : ""}</span>
                <div
                  className={`chart-bar ${allOk ? "healthy" : noneOk ? "unhealthy" : ""}`}
                  style={{ height: `${h}px` }}
                />
                <span className="chart-date">{fmtTime(b.time)}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ─── Service Form (Add / Edit with Advanced Fields) ─────────

interface ServiceFormProps {
  initial?: ServiceConfig;
  onSave: (svc: ServiceConfig) => Promise<void> | void;
  onCancel: () => void;
}

function ServiceForm({ initial, onSave, onCancel, defaultInterval = 30 }: ServiceFormProps & { defaultInterval?: number }) {
  const [name, setName] = useState(initial?.name ?? "");
  const [url, setUrl] = useState(initial?.url ?? "");
  const [method, setMethod] = useState(initial?.method ?? "GET");
  const [interval, setIntervalSecs] = useState(initial?.interval_secs ?? defaultInterval);
  const [timeout, setTimeoutSecs] = useState(initial?.timeout_secs ?? 10);
  const [expectedStatus, setExpectedStatus] = useState(initial?.expected_status ?? 200);
  const [headers, setHeaders] = useState<HeaderPair[]>(initial?.headers ?? []);
  const [body, setBody] = useState(initial?.body ?? "");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [error, setError] = useState("");
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const isEdit = !!initial;

  const handleSubmit = async () => {
    if (savingRef.current) return;
    if (!name.trim()) { setError("请输入服务名称"); return; }
    if (!url.trim()) { setError("请输入 URL"); return; }
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
        setError("仅支持 http/https 协议");
        return;
      }
    } catch { setError("URL 格式无效"); return; }
    setError("");
    savingRef.current = true;
    setSaving(true);
    try {
      await onSave({
        id: initial?.id ?? "",
        name: name.trim(),
        url: url.trim(),
        method,
        interval_secs: Math.max(5, Math.min(300, interval)),
        timeout_secs: Math.max(1, Math.min(120, timeout)),
        expected_status: expectedStatus,
        headers: headers.filter(h => h.key.trim()),
        body: body.trim() || null,
      });
    } catch (e) {
      setError(getErrorMessage(e));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  };

  const handleKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey && !(e.target instanceof HTMLTextAreaElement)) handleSubmit();
    if (e.key === "Escape") onCancel();
  };

  const addHeader = () => setHeaders([...headers, { key: "", value: "" }]);
  const updateHeader = (idx: number, field: "key" | "value", val: string) =>
    setHeaders(headers.map((h, i) => i === idx ? { ...h, [field]: val } : h));
  const removeHeader = (idx: number) => setHeaders(headers.filter((_, i) => i !== idx));

  return (
    <div className="add-form" onKeyDown={handleKey} role="group" aria-label={isEdit ? "编辑服务" : "添加服务"}>
      <input className="form-input" placeholder="服务名称" value={name} onChange={e => { setName(e.target.value); setError(""); }} autoFocus aria-label="服务名称" />
      <input className="form-input" placeholder="https://api.example.com/health" value={url} onChange={e => { setUrl(e.target.value); setError(""); }} aria-label="URL 地址" />
      {error && <span className="form-error" role="alert">{error}</span>}
      <div className="form-row-compact">
        <select className="form-select" value={method} onChange={e => setMethod(e.target.value)} aria-label="HTTP 方法">
          {["GET", "POST", "PUT", "HEAD", "DELETE", "PATCH"].map(m => <option key={m}>{m}</option>)}
        </select>
        <input className="form-input form-input-sm" type="number" min={5} max={300} placeholder="30s" value={interval} onChange={e => setIntervalSecs(Number(e.target.value))} aria-label="检测间隔（秒）" />
        <button className="btn btn-primary btn-xs" onClick={handleSubmit} disabled={saving}>
          {saving ? <RefreshCw className="spinning" style={{ width: 12, height: 12 }} /> : isEdit ? <Check style={{ width: 12, height: 12 }} /> : <Plus style={{ width: 12, height: 12 }} />}
          {saving ? " 处理中" : isEdit ? " 保存" : " 添加"}
        </button>
        <button className="btn btn-secondary btn-xs" onClick={onCancel} disabled={saving}>取消</button>
      </div>

      {/* Advanced toggle */}
      <button className="btn-advanced-toggle" onClick={() => setShowAdvanced(!showAdvanced)} aria-expanded={showAdvanced}>
        <ChevronDown style={{ width: 12, height: 12, transition: "transform 0.2s", transform: showAdvanced ? "rotate(180deg)" : "none" }} />
        {showAdvanced ? "收起高级选项" : "高级选项"}
      </button>

      {showAdvanced && (
        <div className="form-advanced">
          <div className="form-row-compact">
            <div className="form-field">
              <label className="form-label">超时 (秒)</label>
              <input className="form-input form-input-sm" type="number" min={1} max={120} value={timeout} onChange={e => setTimeoutSecs(Number(e.target.value))} aria-label="超时时间" />
            </div>
            <div className="form-field">
              <label className="form-label">期望状态码</label>
              <input className="form-input form-input-sm" type="number" min={100} max={599} value={expectedStatus} onChange={e => setExpectedStatus(Number(e.target.value))} aria-label="期望 HTTP 状态码" />
            </div>
          </div>

          {/* Headers */}
          <div className="form-field">
            <div className="form-label-row">
              <label className="form-label">自定义 Headers</label>
              <button className="btn btn-secondary btn-xs" onClick={addHeader} style={{ padding: "2px 6px", fontSize: 10 }}>
                <Plus style={{ width: 10, height: 10 }} /> 添加
              </button>
            </div>
            {headers.map((h, i) => (
              <div key={i} className="header-row">
                <input className="form-input form-input-sm" placeholder="Key" value={h.key} onChange={e => updateHeader(i, "key", e.target.value)} aria-label={`Header ${i + 1} key`} />
                <input className="form-input" placeholder="Value" value={h.value} onChange={e => updateHeader(i, "value", e.target.value)} style={{ flex: 1 }} aria-label={`Header ${i + 1} value`} />
                <button className="icon-btn-sm danger" onClick={() => removeHeader(i)} title="删除" aria-label={`删除 Header ${i + 1}`}>
                  <X />
                </button>
              </div>
            ))}
          </div>

          {/* Body */}
          {["POST", "PUT", "PATCH"].includes(method) && (
            <div className="form-field">
              <label className="form-label">请求体</label>
              <textarea className="form-input form-textarea" placeholder='{"key": "value"}' value={body} onChange={e => setBody(e.target.value)} aria-label="请求体内容" />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Confirm Dialog ─────────────────────────────────────────

function ConfirmDialog({ message, onConfirm, onCancel, danger }: {
  message: string; onConfirm: () => void; onCancel: () => void; danger?: boolean;
}) {
  const trapRef = useFocusTrap(true);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onCancel(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onCancel]);

  return (
    <div className="confirm-overlay" onClick={onCancel} role="dialog" aria-modal="true" aria-label="确认操作">
      <div className="confirm-card" onClick={e => e.stopPropagation()} ref={trapRef}>
        <AlertTriangle style={{ width: 24, height: 24, color: "var(--orange)", margin: "0 auto" }} />
        <div className="confirm-text">{message}</div>
        <div className="confirm-actions">
          <button className="btn btn-secondary btn-xs" onClick={onCancel}>取消</button>
          <button className="btn btn-primary btn-xs" onClick={onConfirm} style={{ background: danger ? "var(--red)" : "var(--brand)" }}>
            {danger ? "确认删除" : "确认"}
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Settings Panel ─────────────────────────────────────────

interface SettingsPanelProps {
  settings: AppSettings;
  onSave: (s: AppSettings) => void;
  onClose: () => void;
  onClearHistory: () => void;
}

function SettingsPanel({ settings, onSave, onClose, onClearHistory }: SettingsPanelProps) {
  const [notificationsEnabled, setNotif] = useState(settings.notifications_enabled);
  const [autoStart, setAutoStart] = useState(settings.auto_start);
  const [historyDays, setHistoryDays] = useState(settings.history_days);
  const [startMinimized, setStartMinimized] = useState(settings.start_minimized);
  const [defaultInterval, setDefaultInterval] = useState(settings.default_interval_secs);
  const [confirmClear, setConfirmClear] = useState(false);
  const trapRef = useFocusTrap(true);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleSave = () => {
    onSave({
      ...settings,
      notifications_enabled: notificationsEnabled,
      auto_start: autoStart,
      history_days: Math.max(1, Math.min(365, historyDays)),
      start_minimized: startMinimized,
      default_interval_secs: Math.max(5, Math.min(300, defaultInterval)),
    });
  };

  return (
    <div className="settings-overlay" onClick={onClose} role="dialog" aria-modal="true" aria-label="设置">
      <div className="settings-panel" onClick={e => e.stopPropagation()} ref={trapRef}>
        <div className="settings-header">
          <Settings style={{ width: 16, height: 16 }} />
          <span className="settings-title">设置</span>
          <button className="icon-btn" onClick={onClose} title="关闭" aria-label="关闭设置">
            <X />
          </button>
        </div>

        <div className="settings-body">
          <div className="setting-row">
            <div className="setting-info">
              <Bell style={{ width: 14, height: 14 }} className="icon-blue" />
              <span className="setting-label">异常通知</span>
            </div>
            <label className="toggle" aria-label="异常通知开关">
              <input type="checkbox" role="switch" aria-checked={notificationsEnabled} checked={notificationsEnabled} onChange={e => setNotif(e.target.checked)} />
              <span className="toggle-track" />
            </label>
          </div>

          <div className="setting-row">
            <div className="setting-info">
              <Zap style={{ width: 14, height: 14 }} className="icon-orange" />
              <span className="setting-label">开机自启</span>
            </div>
            <label className="toggle" aria-label="开机自启开关">
              <input type="checkbox" role="switch" aria-checked={autoStart} checked={autoStart} onChange={e => setAutoStart(e.target.checked)} />
              <span className="toggle-track" />
            </label>
          </div>

          <div className="setting-row">
            <div className="setting-info">
              <Activity style={{ width: 14, height: 14 }} className="icon-blue" />
              <span className="setting-label">静默启动</span>
            </div>
            <label className="toggle" aria-label="静默启动开关">
              <input type="checkbox" role="switch" aria-checked={startMinimized} checked={startMinimized} onChange={e => setStartMinimized(e.target.checked)} />
              <span className="toggle-track" />
            </label>
          </div>

          <div className="setting-row">
            <div className="setting-info">
              <Clock style={{ width: 14, height: 14 }} className="icon-green" />
              <span className="setting-label">默认间隔</span>
            </div>
            <div className="setting-input-wrap">
              <input
                className="form-input setting-input"
                type="number"
                min={5}
                max={300}
                value={defaultInterval}
                onChange={e => setDefaultInterval(Number(e.target.value))}
                aria-label="默认检测间隔（秒）"
              />
              <span className="setting-unit">秒</span>
            </div>
          </div>

          <div className="setting-row">
            <div className="setting-info">
              <Clock style={{ width: 14, height: 14 }} className="icon-green" />
              <span className="setting-label">历史保留</span>
            </div>
            <div className="setting-input-wrap">
              <input
                className="form-input setting-input"
                type="number"
                min={1}
                max={365}
                value={historyDays}
                onChange={e => setHistoryDays(Number(e.target.value))}
                aria-label="历史保留天数"
              />
              <span className="setting-unit">天</span>
            </div>
          </div>

          <div className="setting-row setting-row-action">
            <div className="setting-info">
              <Trash2 style={{ width: 14, height: 14 }} className="icon-red" />
              <span className="setting-label">清除历史</span>
            </div>
            {confirmClear ? (
              <div className="confirm-inline">
                <span className="confirm-inline-text">确定？</span>
                <button className="btn btn-primary btn-xs" onClick={() => { onClearHistory(); setConfirmClear(false); }} style={{ background: "var(--red)", padding: "4px 8px" }}>确认</button>
                <button className="btn btn-secondary btn-xs" onClick={() => setConfirmClear(false)} style={{ padding: "4px 8px" }}>取消</button>
              </div>
            ) : (
              <button className="btn btn-secondary btn-xs" onClick={() => setConfirmClear(true)}>
                清除全部
              </button>
            )}
          </div>
        </div>

        <div className="settings-footer">
          <button className="btn btn-primary btn-xs" onClick={handleSave} style={{ flex: 1 }}>
            <Check style={{ width: 12, height: 12 }} /> 保存设置
          </button>
        </div>
      </div>
    </div>
  );
}

// ─── Toast Manager ──────────────────────────────────────────

interface Toast { id: number; msg: string; type: "info" | "success" | "error"; exiting?: boolean }

function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const idRef = useRef(0);
  const timersRef = useRef<Set<ReturnType<typeof setTimeout>>>(new Set());

  useEffect(() => {
    return () => {
      for (const t of timersRef.current) clearTimeout(t);
      timersRef.current.clear();
    };
  }, []);

  const addToast = useCallback((msg: string, type: "info" | "success" | "error" = "info") => {
    const id = ++idRef.current;
    setToasts(prev => [...prev.slice(-2), { id, msg, type }]);
    const t1 = setTimeout(() => {
      timersRef.current.delete(t1);
      setToasts(prev => prev.map(t => t.id === id ? { ...t, exiting: true } : t));
      const t2 = setTimeout(() => {
        timersRef.current.delete(t2);
        setToasts(prev => prev.filter(t => t.id !== id));
      }, 260);
      timersRef.current.add(t2);
    }, 2800);
    timersRef.current.add(t1);
  }, []);

  return { toasts, addToast };
}

// ─── Main App ──────────────────────────────────────────────

export default function App() {
  const [services, setServices] = useState<ServiceConfig[]>([]);
  const [statuses, setStatuses] = useState<ServiceStatus[]>([]);
  const [history, setHistory] = useState<CheckRecord[]>([]);
  const [showAdd, setShowAdd] = useState(false);
  const [editingService, setEditingService] = useState<ServiceConfig | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [loading, setLoading] = useState(true);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);
  const [showSettings, setShowSettings] = useState(false);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const { toasts, addToast } = useToasts();
  const [checkingOne, setCheckingOne] = useState<string | null>(null);

  const loadData = useCallback(async () => {
    try {
      const [svcR, stR, histR, cfgR] = await Promise.allSettled([
        invoke<ServiceConfig[]>("get_services"),
        invoke<ServiceStatus[]>("get_all_status"),
        invoke<CheckRecord[]>("get_all_history", { limit: 300 }),
        invoke<AppSettings>("get_settings"),
      ]);
      if (svcR.status === "fulfilled") setServices(svcR.value);
      if (stR.status === "fulfilled") setStatuses(stR.value);
      if (histR.status === "fulfilled") setHistory(histR.value);
      if (cfgR.status === "fulfilled") setSettings(cfgR.value);
      // 如果全部失败才报错
      if ([svcR, stR, histR, cfgR].every(r => r.status === "rejected")) {
        addToast("加载数据失败，请重试", "error");
      }
    } catch (e) {
      console.error(e);
      addToast("加载数据失败，请重试", "error");
    } finally { setLoading(false); }
  }, [addToast]);

  useEffect(() => { loadData(); }, [loadData]);

  const handleRefreshRef = useRef<() => void>(() => {});

  const handleRefresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const r = await invoke<ServiceStatus[]>("check_all");
      setStatuses(r);
      addToast("已刷新全部", "info");
    } catch (e) { addToast(getErrorMessage(e), "error"); }
    finally { setRefreshing(false); }
  }, [addToast]);

  handleRefreshRef.current = handleRefresh;

  const historyDriftRef = useRef(0);

  useEffect(() => {
    const u1 = listen<CheckResult>("check-result", (ev) => {
      setStatuses(prev => prev.map(s =>
        s.service_id === ev.payload.service_id
          ? { ...s, last_check: ev.payload, total_checks: s.total_checks + 1, failed_checks: s.failed_checks + (ev.payload.healthy ? 0 : 1) }
          : s
      ));
      setHistory(prev => [...prev.slice(-299), {
        service_id: ev.payload.service_id,
        healthy: ev.payload.healthy,
        response_time_ms: ev.payload.response_time_ms,
        status: ev.payload.status,
        checked_at: ev.payload.checked_at,
      }]);
      historyDriftRef.current++;
      if (historyDriftRef.current >= 50) {
        historyDriftRef.current = 0;
        invoke<CheckRecord[]>("get_all_history", { limit: 300 }).then(setHistory).catch(() => {});
      }
    });
    const u2 = listen<ServiceAlert>("service-alert", (ev) => {
      addToast(
        ev.payload.type === "down" ? `${ev.payload.name} 已宕机` : `${ev.payload.name} 已恢复`,
        ev.payload.type === "down" ? "error" : "success"
      );
    });
    const u3 = listen("tray-refresh", () => handleRefreshRef.current());
    const u4 = listen<{ all_healthy: boolean; healthy_count: number; total_count: number }>(
      "health-changed",
      (ev) => {
        const { healthy_count, total_count, all_healthy } = ev.payload;
        document.title = all_healthy
          ? `Pulse - ${healthy_count}/${total_count} 正常`
          : `Pulse - ${healthy_count}/${total_count} 异常`;
      }
    );
    return () => {
      u1.then(f => f()).catch(() => {});
      u2.then(f => f()).catch(() => {});
      u3.then(f => f()).catch(() => {});
      u4.then(f => f()).catch(() => {});
    };
  }, [addToast]);

  const handleCheckOne = useCallback(async (id: string) => {
    setCheckingOne(id);
    try {
      const result = await invoke<CheckResult | null>("check_one", { service_id: id });
      if (result) {
        setStatuses(prev => prev.map(s =>
          s.service_id === id
            ? { ...s, last_check: result, total_checks: s.total_checks + 1, failed_checks: s.failed_checks + (result.healthy ? 0 : 1) }
            : s
        ));
      }
    } catch (e) {
      addToast(getErrorMessage(e), "error");
    } finally {
      setCheckingOne(null);
    }
  }, [addToast]);

  const handleAdd = useCallback(async (svc: ServiceConfig) => {
    try {
      const added = await invoke<ServiceConfig>("add_service", { service: svc });
      setServices(p => [...p, added]);
      setShowAdd(false);
      addToast(`已添加 ${added.name}`, "success");
      try {
        const st = await invoke<ServiceStatus[]>("get_all_status");
        setStatuses(st);
      } catch (e) {
        addToast(`刷新状态失败: ${getErrorMessage(e)}`, "error");
      }
    } catch (e) {
      addToast(getErrorMessage(e), "error");
    }
  }, [addToast]);

  const handleEdit = useCallback(async (svc: ServiceConfig) => {
    try {
      await invoke("update_service", { service: svc });
      setServices(p => p.map(s => s.id === svc.id ? svc : s));
      setEditingService(null);
      addToast(`已更新 ${svc.name}`, "success");
      try {
        const st = await invoke<ServiceStatus[]>("get_all_status");
        setStatuses(st);
      } catch (e) {
        addToast(`刷新状态失败: ${getErrorMessage(e)}`, "error");
      }
    } catch (e) {
      addToast(getErrorMessage(e), "error");
    }
  }, [addToast]);

  const handleDelete = useCallback(async (id: string) => {
    try {
      await invoke("remove_service", { service_id: id });
      setServices(p => p.filter(s => s.id !== id));
      setStatuses(p => p.filter(s => s.service_id !== id));
      setDeleteTarget(null);
      addToast("已移除", "info");
    } catch (e) { addToast(getErrorMessage(e), "error"); }
  }, [addToast]);

  const handleSaveSettings = useCallback(async (s: AppSettings) => {
    try {
      await invoke("update_settings", { settings: s });
      setSettings(s);
      setShowSettings(false);
      addToast("设置已保存", "success");
    } catch (e) { addToast(getErrorMessage(e), "error"); }
  }, [addToast]);

  const handleClearHistory = useCallback(async () => {
    try {
      await invoke("clear_history");
      setHistory([]);
      addToast("历史已清除", "success");
    } catch (e) { addToast(getErrorMessage(e), "error"); }
  }, [addToast]);

  // 服务配置 Map（O(1) 查找）
  const servicesMap = useMemo(() => new Map(services.map(s => [s.id, s])), [services]);

  // 预计算每条服务的 sparkline 数据
  const sparkMap = useMemo(() => {
    const m = new Map<string, number[]>();
    for (const r of history) {
      const arr = m.get(r.service_id) ?? [];
      arr.push(r.response_time_ms);
      m.set(r.service_id, arr);
    }
    for (const [k, v] of m) {
      m.set(k, v.slice(-15));
    }
    return m;
  }, [history]);

  // 聚合指标 useMemo
  const { healthyCount, totalCount, downCount, avgMs } = useMemo(() => {
    const healthy = statuses.filter(s => s.last_check?.healthy).length;
    const total = statuses.length;
    const down = statuses.filter(s => s.last_check && !s.last_check.healthy).length;
    const avg = total > 0 ? statuses.reduce((s, v) => s + v.avg_response_ms, 0) / total : 0;
    return { healthyCount: healthy, totalCount: total, downCount: down, avgMs: avg };
  }, [statuses]);

  const deleteName = deleteTarget
    ? statuses.find(s => s.service_id === deleteTarget)?.name ?? servicesMap.get(deleteTarget)?.name ?? "此服务"
    : "";

  return (
    <div className="popup" role="application" aria-label="Pulse 接口监控">
      {/* Header */}
      <div className="popup-header" role="banner">
        <div className="header-brand">
          <Activity className="logo-icon" aria-hidden="true" />
          <span className="logo-name">Pulse</span>
        </div>
        <div className="header-actions">
          <button className={`icon-btn ${refreshing ? "spinning" : ""}`} onClick={handleRefresh} disabled={refreshing} title="刷新全部" aria-label="刷新全部服务">
            <RefreshCw />
          </button>
          <button className="icon-btn" onClick={() => setShowAdd(true)} title="添加服务" aria-label="添加新服务">
            <Plus />
          </button>
          <button className="icon-btn" onClick={() => setShowSettings(true)} title="设置" aria-label="打开设置">
            <Settings />
          </button>
        </div>
      </div>

      {loading ? <LoadingSkeleton /> : (
        <>
          {/* Health Overview Card */}
          <div className={`health-card ${downCount > 0 ? "warn" : ""}`} role="status" aria-label={`接口状态：${healthyCount}/${totalCount} 正常`}>
            <div className="health-card-header">
              <span className="health-card-label">
                <Heart />
                接口状态
              </span>
              <span className={`badge ${totalCount === 0 ? "badge-empty" : downCount > 0 ? "badge-unhealthy" : "badge-healthy"}`}>
                <span className="badge-dot" />
                {totalCount === 0 ? "空闲" : downCount > 0 ? `${downCount} 个异常` : "全部正常"}
              </span>
            </div>
            <div className={`health-amount ${downCount > 0 ? "warn" : ""}`}>
              {totalCount === 0 ? "--" : `${healthyCount}/${totalCount}`}
            </div>
            <div className="health-metrics">
              <div className="compact-metric">
                <span className="compact-metric-label">
                  <Zap className="icon-orange" />
                  平均响应
                </span>
                <span className="compact-metric-value orange">
                  {avgMs > 0 ? formatMs(Math.round(avgMs)) : "--"}
                </span>
              </div>
              <div className="compact-metric">
                <span className="compact-metric-label">
                  <Check className="icon-green" />
                  可用率
                </span>
                <span className="compact-metric-value green">
                  {totalCount > 0 ? `${((healthyCount / totalCount) * 100).toFixed(1)}%` : "--"}
                </span>
              </div>
            </div>
          </div>

          {/* Service list */}
          <div className="service-list" role="list" aria-label="服务列表">
            {statuses.length === 0 && !showAdd ? (
              <div className="empty-msg">
                <Activity style={{ width: 32, height: 32, color: "var(--brand)", opacity: 0.3 }} />
                <span>还没有监控接口</span>
                <button className="btn btn-primary btn-xs" onClick={() => setShowAdd(true)} style={{ marginTop: 4 }}>
                  <Plus style={{ width: 12, height: 12 }} /> 添加第一个服务
                </button>
              </div>
            ) : statuses.map((s, idx) => {
              const checked = s.last_check !== null;
              const healthy = s.last_check?.healthy;
              const iconClass = !checked ? "pending" : healthy ? "up" : "down";
              const color = !checked ? "#FF9500" : healthy ? "#34C759" : "#FF3B30";
              const StatusIcon = !checked ? HelpCircle : healthy ? CircleCheck : AlertTriangle;
              const sparkData = sparkMap.get(s.service_id) ?? [];
              const svcConfig = servicesMap.get(s.service_id);

              return (
                <div key={s.service_id} className="svc-row" style={{ animationDelay: `${idx * 0.04}s` }} role="listitem" tabIndex={0} aria-label={`${s.name} ${healthy ? "正常" : checked ? "异常" : "待检测"}`}>
                  <div className={`svc-icon-circle ${iconClass}`}>
                    <StatusIcon />
                  </div>
                  <div className="svc-info">
                    <span className="svc-name">{s.name}</span>
                    <span className="svc-url">{s.url}</span>
                  </div>
                  <div className="svc-metrics">
                    <div className="svc-spark">
                      <Sparkline data={sparkData} healthy={healthy} />
                    </div>
                    <span className="svc-response" style={{ color }}>
                      {s.last_check ? formatMs(s.last_check.response_time_ms) : "--"}
                    </span>
                    <div className="svc-progress-track">
                      <div
                        className={`svc-progress-fill ${healthy ? "up" : "down"}`}
                        style={{ width: `${s.uptime_pct}%` }}
                      />
                    </div>
                    <span className="svc-uptime">{s.uptime_pct.toFixed(0)}%</span>
                  </div>
                  <div className="svc-actions">
                    <button className="icon-btn-sm" onClick={() => handleCheckOne(s.service_id)} disabled={checkingOne === s.service_id} title="检测" aria-label={`检测 ${s.name}`}>
                      <RotateCcw />
                    </button>
                    <button className="icon-btn-sm edit" onClick={() => svcConfig && setEditingService(svcConfig)} title="编辑" aria-label={`编辑 ${s.name}`}>
                      <Pencil />
                    </button>
                    <button className="icon-btn-sm danger" onClick={() => setDeleteTarget(s.service_id)} title="移除" aria-label={`移除 ${s.name}`}>
                      <X />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          {/* Trend Chart */}
          <TrendChart history={history} />
        </>
      )}

      {/* Add service form */}
      {showAdd && <ServiceForm onSave={handleAdd} onCancel={() => setShowAdd(false)} defaultInterval={settings?.default_interval_secs ?? 30} />}

      {/* Edit service form */}
      {editingService && (
        <ServiceForm
          initial={editingService}
          onSave={handleEdit}
          onCancel={() => setEditingService(null)}
          defaultInterval={settings?.default_interval_secs ?? 30}
        />
      )}

      {/* Delete confirmation */}
      {deleteTarget && (
        <ConfirmDialog
          message={`确定要移除「${deleteName}」吗？`}
          onConfirm={() => handleDelete(deleteTarget)}
          onCancel={() => setDeleteTarget(null)}
          danger
        />
      )}

      {/* Settings panel */}
      {showSettings && settings && (
        <SettingsPanel
          settings={settings}
          onSave={handleSaveSettings}
          onClose={() => setShowSettings(false)}
          onClearHistory={handleClearHistory}
        />
      )}

      {/* Toasts */}
      <div className="toast-strip" aria-live="polite">
        {toasts.map(t => (
          <div key={t.id} className={`toast-pill toast-${t.type} ${t.exiting ? "exiting" : ""}`}>
            {t.msg}
          </div>
        ))}
      </div>
    </div>
  );
}
