import { FolderOpen, RefreshCw, X } from "lucide-react";

interface Props {
  title: string;
  error: string | null;
  onRetry: () => void;
  onClose: () => void;
  onOpenLogs: () => void;
}

// Kept outside the dashboard-dependent UI: loading and failure must both leave
// an actionable window even when there is no configuration to render.
export function WindowStartup({ title, error, onRetry, onClose, onOpenLogs }: Props) {
  return (
    <section className="window-startup" role="dialog" aria-label={title}>
      <div className="window-startup-card">
        <button className="modal-close" aria-label="关闭窗口" title="关闭" onClick={onClose}><X /></button>
        <h2>{title}</h2>
        {error ? (
          <>
            <p className="window-startup-error" role="alert">{error}</p>
            <div className="window-startup-actions">
              <button className="button primary" onClick={onRetry}><RefreshCw /> 重试</button>
              <button className="button secondary" onClick={onOpenLogs}><FolderOpen /> 查看日志</button>
              <button className="button ghost" onClick={onClose}>关闭</button>
            </div>
          </>
        ) : (
          <div className="window-startup-loading" role="status"><RefreshCw className="spin" /> 正在加载，请稍候…</div>
        )}
      </div>
    </section>
  );
}
