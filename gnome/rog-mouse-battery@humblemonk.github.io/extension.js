// Top-bar indicator: runs `rogctl watch --json --device ...` and shows an icon
// and level for each device in each line. Also sends each low-battery
// notification when rogctl flags one.

import Clutter from "gi://Clutter";
import GLib from "gi://GLib";
import GObject from "gi://GObject";
import Gio from "gi://Gio";
import St from "gi://St";

import {
  Extension,
  gettext as _,
} from "resource:///org/gnome/shell/extensions/extension.js";
import * as Main from "resource:///org/gnome/shell/ui/main.js";
import * as PanelMenu from "resource:///org/gnome/shell/ui/panelMenu.js";
import * as PopupMenu from "resource:///org/gnome/shell/ui/popupMenu.js";

// Start rogctl again this long after it exits, e.g. if it wasn't on PATH yet.
const RESTART_SECONDS = 10;
// Restart rogctl for a changed setting once the settings stop changing for
// this long: a spinner changes its setting on every step.
const SETTLE_MS = 1000;
const SIGTERM = 15;

// Status words as the kernel and desktops use them: discharging, charging,
// full. Solaar says "recharging"; rogctl doesn't.
function statusWord(s) {
  if (s.charging && s.battery === 100) return _("full");
  return s.charging ? _("charging") : _("discharging");
}

// "79% (discharging)"; "(offline)" replaces the status while the device sleeps.
function batteryValue(s) {
  if (s.battery === undefined)
    return s.state === "disconnected" ? _("unknown") : _("offline");
  const word = s.state === "connected" ? statusWord(s) : _("offline");
  return _("%d%% (%s)").format(s.battery, word);
}

function deviceName(s) {
  return s.device || (s.kind === "keyboard" ? _("Keyboard") : _("Mouse"));
}

function iconName(s) {
  if (s.state === "error") return "dialog-warning-symbolic";
  if (s.charging && s.battery !== undefined) {
    if (s.battery === 100) return "battery-level-100-charged-symbolic";
    // Rounded down, as GNOME Shell's power indicator does: Adwaita has no
    // battery-level-100-charging icon, so 95-99% would show none.
    return `battery-level-${Math.floor(s.battery / 10) * 10}-charging-symbolic`;
  }
  return s.kind === "keyboard"
    ? "input-keyboard-symbolic"
    : "input-mouse-symbolic";
}

// `--device all` prints `{ devices, pid }`; mouse or keyboard one status,
// which is left out of the list while disconnected.
function report(line) {
  if (line.devices !== undefined) return line;
  return {
    devices: line.state === "disconnected" ? [] : [line],
    pid: line.pid,
  };
}

// The `--device` value for the setting: all, mouse or keyboard.
function selection(settings) {
  const value = settings.get_string("device");
  return value === "mouse" || value === "keyboard" ? value : "all";
}

const Indicator = GObject.registerClass(
  class Indicator extends PanelMenu.Button {
    _init(extension) {
      super._init(0.5, _("ASUS Peripheral Battery"));
      this._extension = extension;
      this._settings = extension.getSettings();
      // The last line as `{ devices, pid }`, or `{ devices: [], error }`
      // while rogctl can't run.
      this._report = null;
      this._proc = null;
      // The pid the running rogctl printed; null until its first line.
      this._pid = null;
      this._reading = null;
      this._restartId = 0;
      this._settleId = 0;
      this._cancellable = new Gio.Cancellable();

      // An icon and label per device, rebuilt on each line.
      this._box = new St.BoxLayout({ style_class: "panel-status-menu-box" });
      this.add_child(this._box);

      // A name and battery line per device.
      this._details = new PopupMenu.PopupMenuSection();
      this.menu.addMenuItem(this._details);
      this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
      this.menu.addAction(_("Refresh"), () => this._refresh());
      this.menu.addAction(_("Settings"), () =>
        this._extension.openPreferences(),
      );

      this._settingsIds = [
        this._settings.connect("changed::binary", () =>
          this._restartWhenSettled(),
        ),
        this._settings.connect("changed::device", () =>
          this._restartWhenSettled(),
        ),
        this._settings.connect("changed::interval", () =>
          this._restartWhenSettled(),
        ),
        this._settings.connect("changed::show-percent", () => this._render()),
        this._settings.connect("changed::low-threshold", () =>
          this._restartWhenSettled(),
        ),
        this._settings.connect("changed::hide-when-disconnected", () =>
          this._render(),
        ),
      ];

      this._render();
      this._startWatch();
    }

    get _binary() {
      return this._settings.get_string("binary") || "rogctl";
    }

    _startWatch() {
      this._pid = null;
      // Refresh goes through the same binary, so a wrapper's pid means the
      // same there.
      this._watchBinary = this._binary;
      const argv = [
        this._binary,
        "watch",
        "--json",
        "--device",
        selection(this._settings),
        "--interval",
        String(this._settings.get_int("interval")),
        "--low-threshold",
        String(this._settings.get_int("low-threshold")),
      ];
      let proc;
      try {
        proc = Gio.Subprocess.new(
          argv,
          Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_SILENCE,
        );
      } catch (e) {
        this._publishError(
          _("Could not start %s: %s").format(this._binary, e.message),
        );
        this._scheduleRestart();
        return;
      }
      this._proc = proc;
      // Cancelled when this rogctl is stopped, so its lines stop counting.
      this._reading = new Gio.Cancellable();

      const stream = new Gio.DataInputStream({
        base_stream: proc.get_stdout_pipe(),
        close_base_stream: true,
      });
      this._readLine(stream, this._reading);

      proc.wait_async(this._cancellable, (p, res) => {
        try {
          p.wait_finish(res);
        } catch {
          return; // Cancelled: the indicator is being destroyed.
        }
        if (this._proc !== p) return; // Stopped by _restart(), which starts the next one.
        this._proc = null;
        this._pid = null;
        this._publishError(
          p.get_if_exited()
            ? _("%s exited with status %d").format(
                this._binary,
                p.get_exit_status(),
              )
            : _("%s was killed by signal %d").format(
                this._binary,
                p.get_term_sig(),
              ),
        );
        this._scheduleRestart();
      });
    }

    _readLine(stream, cancellable) {
      stream.read_line_async(GLib.PRIORITY_DEFAULT, cancellable, (s, res) => {
        let line;
        try {
          [line] = s.read_line_finish_utf8(res);
        } catch {
          // Stopped or closed. Closing the pipe makes a rogctl that outlived
          // its stop (under a wrapper script SIGTERM only reaches the
          // script) exit at its next line.
          s.close_async(GLib.PRIORITY_DEFAULT, null, null);
          return;
        }
        if (line === null) return; // rogctl exited; wait_async handles that.
        this._handleLine(line);
        this._readLine(s, cancellable);
      });
    }

    _scheduleRestart() {
      if (this._restartId) return;
      this._restartId = GLib.timeout_add_seconds(
        GLib.PRIORITY_DEFAULT,
        RESTART_SECONDS,
        () => {
          this._restartId = 0;
          this._startWatch();
          return GLib.SOURCE_REMOVE;
        },
      );
    }

    _stopWatch() {
      if (this._restartId) {
        GLib.source_remove(this._restartId);
        this._restartId = 0;
      }
      const proc = this._proc;
      this._proc = null;
      this._pid = null;
      this._reading?.cancel();
      this._reading = null;
      proc?.send_signal(SIGTERM);
    }

    _restart() {
      this._stopWatch();
      this._startWatch();
    }

    _restartWhenSettled() {
      this._cancelSettle();
      this._settleId = GLib.timeout_add(
        GLib.PRIORITY_DEFAULT,
        SETTLE_MS,
        () => {
          this._settleId = 0;
          this._restart();
          return GLib.SOURCE_REMOVE;
        },
      );
    }

    _cancelSettle() {
      if (this._settleId) {
        GLib.source_remove(this._settleId);
        this._settleId = 0;
      }
    }

    // Read the devices now instead of waiting for the next interval, once this
    // rogctl printed its pid (until then the signal would kill it). `rogctl
    // refresh` signals the pid only if it's a `rogctl watch`, unlike `kill`.
    _refresh() {
      if (!this._pid) return;
      try {
        Gio.Subprocess.new(
          [this._watchBinary, "refresh", String(this._pid)],
          Gio.SubprocessFlags.STDERR_SILENCE,
        );
      } catch (e) {
        console.warn(`rog-mouse-battery: ${e.message}`);
      }
    }

    _handleLine(line) {
      let r;
      try {
        r = report(JSON.parse(line));
      } catch {
        console.warn(`rog-mouse-battery: bad line from rogctl: ${line}`);
        return;
      }
      this._report = r;
      if (r.pid) this._pid = r.pid;
      for (const s of r.devices) {
        if (!s.notify_low) continue;
        // Solaar's format: the device name as the title, its battery line as the body.
        Main.notify(
          deviceName(s),
          _("Battery: %d%% (%s)").format(s.battery, statusWord(s)),
        );
      }
      this._render();
    }

    _publishError(message) {
      this._report = { devices: [], error: message };
      this._render();
    }

    // An icon, with the level if it's known and shown, in its own box so its
    // colour and dimming are its own.
    _addEntry(icon, s) {
      const entry = new St.BoxLayout();
      entry.add_child(
        new St.Icon({ icon_name: icon, style_class: "system-status-icon" }),
      );
      const showPercent = this._settings.get_boolean("show-percent");
      if (s && s.battery !== undefined && showPercent)
        entry.add_child(
          new St.Label({
            text: `${s.battery}%`,
            y_align: Clutter.ActorAlign.CENTER,
          }),
        );
      if (s?.low) entry.add_style_class_name("rog-mouse-battery-low");
      if (!s || s.state === "error")
        entry.add_style_class_name("rog-mouse-battery-error");
      // Asleep: the level shown is the last one read.
      entry.opacity = !s || s.state !== "asleep" ? 255 : 128;
      this._box.add_child(entry);
    }

    _addDetail(text) {
      this._details.addMenuItem(
        new PopupMenu.PopupMenuItem(text, { reactive: false }),
      );
    }

    _render() {
      const r = this._report;
      this._box.destroy_all_children();
      this._details.removeAll();
      if (!r) {
        this._box.add_child(
          new St.Icon({
            icon_name: "input-mouse-symbolic",
            style_class: "system-status-icon",
          }),
        );
        this._addDetail(_("ASUS Peripheral Battery"));
        this._addDetail(_("Waiting for rogctl…"));
        return;
      }

      const devices = r.devices;
      this.visible = !(
        r.error === undefined &&
        devices.length === 0 &&
        this._settings.get_boolean("hide-when-disconnected")
      );

      if (r.error !== undefined) {
        this._addEntry("dialog-warning-symbolic", null);
        this._addDetail(_("Error: %s").format(r.error));
        return;
      }
      if (devices.length === 0) {
        const icon = new St.Icon({
          icon_name: "dialog-warning-symbolic",
          style_class: "system-status-icon",
          opacity: 128,
        });
        this._box.add_child(icon);
        this._addDetail(_("Not connected"));
        this._addDetail(_("Battery: %s").format(_("unknown")));
        return;
      }
      for (const s of devices) {
        this._addEntry(iconName(s), s);
        this._addDetail(deviceName(s));
        this._addDetail(
          s.state === "error"
            ? _("Error: %s").format(s.error || "")
            : _("Battery: %s").format(batteryValue(s)),
        );
      }
    }

    destroy() {
      this._cancellable.cancel();
      this._cancelSettle();
      this._stopWatch();
      for (const id of this._settingsIds) this._settings.disconnect(id);
      this._settingsIds = [];
      this._settings = null;
      super.destroy();
    }
  },
);

export default class RogMouseBatteryExtension extends Extension {
  enable() {
    this._indicator = new Indicator(this);
    Main.panel.addToStatusArea(this.uuid, this._indicator);
  }

  disable() {
    this._indicator?.destroy();
    this._indicator = null;
  }
}
