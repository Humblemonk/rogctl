import Adw from "gi://Adw";
import Gio from "gi://Gio";
import Gtk from "gi://Gtk";

import {
  ExtensionPreferences,
  gettext as _,
} from "resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js";

export default class RogMouseBatteryPreferences extends ExtensionPreferences {
  fillPreferencesWindow(window) {
    const settings = this.getSettings();
    const page = new Adw.PreferencesPage();
    const group = new Adw.PreferencesGroup();
    page.add(group);

    const binary = new Adw.EntryRow({ title: _("rogctl command") });
    settings.bind("binary", binary, "text", Gio.SettingsBindFlags.DEFAULT);
    group.add(binary);

    const interval = new Adw.SpinRow({
      title: _("Refresh seconds"),
      subtitle: _("How often to read the mouse battery."),
      adjustment: new Gtk.Adjustment({
        lower: 5,
        upper: 600,
        step_increment: 5,
      }),
    });
    settings.bind("interval", interval, "value", Gio.SettingsBindFlags.DEFAULT);
    group.add(interval);

    const threshold = new Adw.SpinRow({
      title: _("Low battery warning (%)"),
      subtitle: _(
        "Notify and turn the indicator red at or below this level. 0 disables it.",
      ),
      adjustment: new Gtk.Adjustment({
        lower: 0,
        upper: 100,
        step_increment: 5,
      }),
    });
    settings.bind(
      "low-threshold",
      threshold,
      "value",
      Gio.SettingsBindFlags.DEFAULT,
    );
    group.add(threshold);

    const showPercent = new Adw.SwitchRow({ title: _("Show percentage") });
    settings.bind(
      "show-percent",
      showPercent,
      "active",
      Gio.SettingsBindFlags.DEFAULT,
    );
    group.add(showPercent);

    const hide = new Adw.SwitchRow({
      title: _("Hide when no mouse is connected"),
      subtitle: _(
        "Hide the indicator while the receiver or cable is unplugged.",
      ),
    });
    settings.bind(
      "hide-when-disconnected",
      hide,
      "active",
      Gio.SettingsBindFlags.DEFAULT,
    );
    group.add(hide);

    window.add(page);
  }
}
