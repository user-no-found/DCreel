import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke, Channel: class {} }));

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

describe("hotkey capture request order", () => {
  beforeEach(() => {
    vi.resetModules();
    invoke.mockReset();
    vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("keeps rapid focus, blur and focus in order while native IPC is busy", async () => {
    const first = deferred();
    invoke.mockReturnValueOnce(first.promise).mockResolvedValue(undefined);
    const { setHotkeyCaptureActive } = await import("./bridge");
    const focused = setHotkeyCaptureActive(true);
    const blurred = setHotkeyCaptureActive(false);
    const refocused = setHotkeyCaptureActive(true);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    expect(invoke).toHaveBeenNthCalledWith(1, "set_hotkey_capture_active", { active: true });
    first.resolve();
    await Promise.all([focused, blurred, refocused]);
    expect(invoke).toHaveBeenNthCalledWith(2, "set_hotkey_capture_active", { active: false });
    expect(invoke).toHaveBeenNthCalledWith(3, "set_hotkey_capture_active", { active: true });
  });

  it("reports a failed capture and still restores capture on blur", async () => {
    const first = deferred();
    invoke.mockReturnValueOnce(first.promise).mockResolvedValue(undefined);
    const { setHotkeyCaptureActive } = await import("./bridge");
    const focused = setHotkeyCaptureActive(true);
    const failure = expect(focused).rejects.toThrow("host unavailable");
    const blurred = setHotkeyCaptureActive(false);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    first.reject(new Error("host unavailable"));
    await failure;
    await blurred;
    const captureCalls = invoke.mock.calls.filter(([command]) => command === "set_hotkey_capture_active");
    expect(captureCalls.map(([, args]) => args.active)).toEqual([true, false]);
    expect(invoke).toHaveBeenCalledWith("write_frontend_log", expect.objectContaining({ level: "error" }));
  });
});
