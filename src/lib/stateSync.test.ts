import { describe, expect, it } from "vitest";
import type { Dashboard, Preferences } from "../types";
import { clearMatchingPatch, mergeDashboardWithOptimistic } from "./stateSync";

const preferences: Preferences = {
  titleOpacity: 0.9,
  contentOpacity: 0.9,
  showFenceBorder: true,
  fenceBorderOpacity: 0.4,
  iconSize: 46,
  defaultFenceWidth: 330,
  defaultFenceHeight: 280,
  ghostMode: false,
  ghostModeTrigger: "automatic",
  ghostOpacity: 0.2,
  ghostHotkey: "Ctrl+Alt+G",
  startOnBoot: false,
  showHiddenFiles: false,
  showFenceTitles: true,
  showTrayIcon: true,
  desktopMode: true,
  desktopContextMenu: true
};

const dashboard: Dashboard = {
  desktopVisible: false,
  preferences,
  fences: [
    {
      id: "fence-1",
      title: "磁盘中的新名称",
      directory: "C:\\Data",
      x: 1880,
      y: 20,
      width: 420,
      height: 300,
      color: "coral",
      contentColor: "paper",
      collapsed: false,
      locked: false,
      itemCount: 42
    }
  ]
};

describe("optimistic state synchronization", () => {
  it("keeps a newer value when an older request completes", () => {
    expect(clearMatchingPatch<Preferences>({ titleOpacity: 0.25 }, { titleOpacity: 0.6 })).toEqual({
      titleOpacity: 0.25
    });
  });

  it("removes only values confirmed by the completed request", () => {
    expect(
      clearMatchingPatch<Preferences>(
        { titleOpacity: 0.6, contentOpacity: 0.4 },
        { titleOpacity: 0.6 }
      )
    ).toEqual({ contentOpacity: 0.4 });
  });

  it("merges pending appearance changes without replacing fresh host geometry", () => {
    const merged = mergeDashboardWithOptimistic(
      dashboard,
      { iconSize: 58 },
      new Map([["fence-1", { color: "sage" }]])
    );

    expect(merged.desktopVisible).toBe(false);
    expect(merged.preferences.iconSize).toBe(58);
    expect(merged.fences[0]).toMatchObject({
      color: "sage",
      x: 1880,
      y: 20,
      width: 420,
      height: 300
    });
  });
});
