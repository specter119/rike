#!/usr/bin/env python3
"""A plain GTK4 TextView: the control group of wayland-ime-smoke.sh.

If Chinese input fails here too, the harness or the session is at fault, not
the application under test.
"""

import os

import gi

gi.require_version("Gtk", "4.0")
from gi.repository import GLib, Gtk  # noqa: E402

# GTK4_TEXTVIEW_DELAY_MS: create the TextView only after the window has had
# the keyboard focus for a while, as NSH does when an input mounts.
DELAY_MS = int(os.environ.get("GTK4_TEXTVIEW_DELAY_MS", "0"))


def activate(app):
    window = Gtk.ApplicationWindow(application=app, title="gtk4-textview")
    window.set_default_size(640, 300)
    window.present()

    def add_view():
        view = Gtk.TextView(wrap_mode=Gtk.WrapMode.WORD)
        view.set_vexpand(True)
        window.set_child(view)
        view.grab_focus()
        return False

    if DELAY_MS:
        GLib.timeout_add(DELAY_MS, add_view)
    else:
        add_view()


app = Gtk.Application(application_id="dev.rike.gtk4textview")
app.connect("activate", activate)
app.run(None)
