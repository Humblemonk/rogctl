.pragma library

// Battery wording shared by the daemon and widget, following Solaar like the
// other rogctl widgets. `tr` is I18n.trFor bound to this plugin, since a
// library can't see the shell's singletons.

// Solaar's status words: discharging, recharging, full.
function statusWord(s, tr) {
    if (s.charging && s.battery === 100)
        return tr("full");
    if (s.charging)
        return tr("recharging");
    return tr("discharging");
}

// "79% (discharging)"; "(offline)" replaces the status while the mouse sleeps.
function value(s, tr) {
    if (s.battery === undefined)
        return s.state === "disconnected" ? tr("unknown") : tr("offline");
    const word = s.state === "connected" ? statusWord(s, tr) : tr("offline");
    return tr("%1% (%2)").arg(s.battery).arg(word);
}

// "Battery: 79% (discharging)", Solaar's notification body.
function line(s, tr) {
    return tr("Battery: %1% (%2)").arg(s.battery).arg(statusWord(s, tr));
}

// Material Symbols name for the bar icon.
function icon(s) {
    if (s.state === "error" || s.state === "disconnected")
        return "error";
    return s.charging ? "battery_charging_full" : "mouse";
}
