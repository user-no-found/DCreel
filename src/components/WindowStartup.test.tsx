import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { WindowStartup } from "./WindowStartup";

describe("window startup fallback", () => {
  const actions = { onRetry() {}, onClose() {}, onOpenLogs() {} };
  it("keeps a close button while loading without a dashboard", () => {
    const html = renderToStaticMarkup(<WindowStartup title="新建盒子" error={null} {...actions} />);
    expect(html).toContain('aria-label="关闭窗口"');
    expect(html).toContain('role="status"');
  });
  it("shows the error, retry, logs and close actions", () => {
    const html = renderToStaticMarkup(<WindowStartup title="新建盒子" error="配置读取失败" {...actions} />);
    for (const text of ["配置读取失败", "重试", "查看日志", 'aria-label="关闭窗口"']) expect(html).toContain(text);
    expect(html).not.toContain('role="status"');
  });
});
