// Top-bar indicator: runs `rogctl watch --json` and shows each status line.
// Also sends the low-battery notification when rogctl flags one.

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
const SIGUSR1 = 10;
const SIGTERM = 15;

// Status words as the kernel and desktops use them: discharging, charging,
// full. Solaar says "recharging"; rogctl doesn't.
function statusWord(s) {
  if (s.charging && s.battery === 100) return _("full");
  return s.charging ? _("charging") : _("discharging");
}

// "79% (discharging)"; "(offline)" replaces the status while the mouse sleeps.
function batteryValue(s) {
  if (s.battery === undefined)
    return s.state === "disconnected" ? _("unknown") : _("offline");
  const word = s.state === "connected" ? statusWord(s) : _("offline");
  return _("%d%% (%s)").format(s.battery, word);
}

function iconName(s) {
  if (s.state === "error" || s.state === "disconnected")
    return "dialog-warning-symbolic";
  if (s.charging && s.battery !== undefined) {
    if (s.battery === 100) return "battery-level-100-charged-symbolic";
    return `battery-level-${Math.round(s.battery / 10) * 10}-charging-symbolic`;
  }
  return "input-mouse-symbolic";
}

const Indicator = GObject.registerClass(
  class Indicator extends PanelMenu.Button {
    _init(extension) {
      super._init(0.5, _("ASUS Mouse Battery"));
      this._extension = extension;
      this._settings = extension.getSettings();
      this._status = null;
      this._proc = null;
      this._restartId = 0;
      this._cancellable = new Gio.Cancellable();

      this._box = new St.BoxLayout({ style_class: "panel-status-menu-box" });
      this._icon = new St.Icon({
        icon_name: "input-mouse-symbolic",
        style_class: "system-status-icon",
      });
      this._label = new St.Label({ y_align: Clutter.ActorAlign.CENTER });
      this._box.add_child(this._icon);
      this._box.add_child(this._label);
      this.add_child(this._box);

      this._deviceItem = new PopupMenu.PopupMenuItem("", { reactive: false });
      this._detailItem = new PopupMenu.PopupMenuItem("", { reactive: false });
      this.menu.addMenuItem(this._deviceItem);
      this.menu.addMenuItem(this._detailItem);
      this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
      this.menu.addAction(_("Refresh"), () => this._refresh());
      this.menu.addAction(_("Settings"), () =>
        this._extension.openPreferences(),
      );

      this._settingsIds = [
        this._settings.connect("changed::binary", () => this._restart()),
        this._settings.connect("changed::interval", () => this._restart()),
        this._settings.connect("changed::show-percent", () => this._render()),
        this._settings.connect("changed::low-threshold", () => this._restart()),
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
      const argv = [
        this._binary,
        "watch",
        "--json",
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

      const stream = new Gio.DataInputStream({
        base_stream: proc.get_stdout_pipe(),
        close_base_stream: true,
      });
      this._readLine(stream);

      proc.wait_async(this._cancellable, (p, res) => {
        try {
          p.wait_finish(res);
        } catch {
          return; // Cancelled: the indicator is being destroyed.
        }
        if (this._proc !== p) return; // Stopped by _restart(), which starts the next one.
        this._proc = null;
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

    _readLine(stream) {
      stream.read_line_async(
        GLib.PRIORITY_DEFAULT,
        this._cancellable,
        (s, res) => {
          let line;
          try {
            [line] = s.read_line_finish_utf8(res);
          } catch {
            return; // Cancelled or closed.
          }
          if (line === null) return; // rogctl exited; wait_async handles that.
          this._handleLine(line);
          this._readLine(s);
        },
      );
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
      proc?.send_signal(SIGTERM);
    }

    _restart() {
      this._stopWatch();
      this._startWatch();
    }

    // Read the mouse now instead of waiting for the next interval: SIGUSR1
    // makes the running rogctl read it right away.
    _refresh() {
      this._proc?.send_signal(SIGUSR1);
    }

    _handleLine(line) {
      let s;
      try {
        s = JSON.parse(line);
      } catch {
        console.warn(`rog-mouse-battery: bad line from rogctl: ${line}`);
        return;
      }
      this._status = s;
      if (s.notify_low) {
        // Solaar's format: the device name as the title, its battery line as the body.
        Main.notify(
          s.device || _("Mouse"),
          _("Battery: %d%% (%s)").format(s.battery, statusWord(s)),
        );
      }
      this._render();
    }

    _publishError(message) {
      this._status = { state: "error", charging: false, error: message };
      this._render();
    }

    _render() {
      const s = this._status;
      if (!s) {
        this._label.text = "";
        this._deviceItem.label.text = _("ASUS Mouse Battery");
        this._detailItem.label.text = _("Waiting for rogctl…");
        return;
      }

      this.visible = !(
        s.state === "disconnected" &&
        this._settings.get_boolean("hide-when-disconnected")
      );

      this._icon.icon_name = iconName(s);
      this._label.text =
        s.battery !== undefined && this._settings.get_boolean("show-percent")
          ? `${s.battery}%`
          : "";

      for (const [name, on] of [
        ["low", s.low],
        ["error", s.state === "error"],
      ]) {
        const cls = `rog-mouse-battery-${name}`;
        if (on) this._box.add_style_class_name(cls);
        else this._box.remove_style_class_name(cls);
      }
      // Asleep or gone: the level shown is the last one read.
      this._box.opacity =
        s.state === "connected" || s.state === "error" ? 255 : 128;

      this._deviceItem.label.text = s.device || _("Not connected");
      this._detailItem.label.text =
        s.state === "error"
          ? _("Error: %s").format(s.error || "")
          : _("Battery: %s").format(batteryValue(s));
    }

    destroy() {
      this._cancellable.cancel();
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
