param(
  [string]$ProcessName = "dcreel",
  [switch]$DisturbLayer
)

$source = @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class CreelWindowInspector
{
    public delegate bool EnumProc(IntPtr hwnd, IntPtr lparam);

    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumProc callback, IntPtr lparam);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);

    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);

    [DllImport("user32.dll")]
    private static extern bool SetWindowPos(
        IntPtr hwnd, IntPtr insertAfter, int x, int y, int width, int height, uint flags
    );

    [StructLayout(LayoutKind.Sequential)]
    private struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    public static string[] Inspect(int processId)
    {
        var rows = new List<string>();
        int zIndex = 0;
        EnumWindows((hwnd, _) =>
        {
            int currentZ = zIndex++;
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            var className = new StringBuilder(128);
            GetClassName(hwnd, className, className.Capacity);
            bool desktopHost = className.ToString() == "Progman" || className.ToString() == "WorkerW";
            if (owner != (uint)processId && !desktopHost)
            {
                return true;
            }

            var title = new StringBuilder(256);
            GetWindowText(hwnd, title, title.Capacity);
            Rect rect;
            GetWindowRect(hwnd, out rect);
            long extendedStyle = GetWindowLongPtr(hwnd, -20).ToInt64();
            rows.Add(String.Format(
                "z={0} HWND=0x{1:X} visible={2} rect={3},{4},{5}x{6} ex=0x{7:X} class={8} title={9}",
                currentZ, hwnd.ToInt64(), IsWindowVisible(hwnd), rect.Left, rect.Top,
                rect.Right - rect.Left, rect.Bottom - rect.Top, extendedStyle,
                className, title
            ));
            return true;
        }, IntPtr.Zero);
        return rows.ToArray();
    }

    public static bool RaiseFirstFence(int processId)
    {
        bool raised = false;
        EnumWindows((hwnd, _) =>
        {
            uint owner;
            GetWindowThreadProcessId(hwnd, out owner);
            long extendedStyle = GetWindowLongPtr(hwnd, -20).ToInt64();
            if (owner == (uint)processId && (extendedStyle & 0x80) != 0 && IsWindowVisible(hwnd))
            {
                raised = SetWindowPos(hwnd, IntPtr.Zero, 0, 0, 0, 0, 0x13);
                return false;
            }
            return true;
        }, IntPtr.Zero);
        return raised;
    }
}
'@

if (-not ("CreelWindowInspector" -as [type])) {
  Add-Type -TypeDefinition $source
}

$processes = @(Get-Process -Name $ProcessName -ErrorAction Stop)
foreach ($process in $processes) {
  Write-Output "Process $($process.Id):"
  [CreelWindowInspector]::Inspect($process.Id)
  if ($DisturbLayer) {
    Write-Output "Temporarily moving one fence to HWND_TOP..."
    [void][CreelWindowInspector]::RaiseFirstFence($process.Id)
    [CreelWindowInspector]::Inspect($process.Id)
    Start-Sleep -Seconds 4
    Write-Output "After the DCreel desktop-layer watchdog:"
    [CreelWindowInspector]::Inspect($process.Id)
  }
}
