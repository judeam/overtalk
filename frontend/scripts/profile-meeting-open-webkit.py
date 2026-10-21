"""Run the synthetic meeting-open probe in Linux's native WebKitGTK engine.

Requires the system Python's PyGObject, GTK 3 and WebKit2 4.1 packages.
Serve the production frontend before running this script. No real app data is used.
"""

import argparse
import json
from pathlib import Path
import subprocess

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--base", default="http://127.0.0.1:3118/")
parser.add_argument("--summary-delay", type=int, default=500)
parser.add_argument("--settle-ms", type=int, default=0)
args = parser.parse_args()
probe = str(Path(__file__).with_name("profile-meeting-open.cjs"))
source = subprocess.check_output([
    "node", "-e", """
    const p = require(process.argv[1]);
    for (const name of ['installFixture', 'supplement', 'updatedFixture'])
      console.log(`(${p[name]})();`);
    console.log(`(${p.meetingFixture})({summaryDelay:${Number(process.argv[2])}});`);
    console.log(`window.measureMeetingOpen = ${p.measureMeetingOpen};`);
    """, probe, str(args.summary_delay),
], text=True)

manager = WebKit2.UserContentManager()
manager.add_script(WebKit2.UserScript.new(
    source, WebKit2.UserContentInjectedFrames.TOP_FRAME,
    WebKit2.UserScriptInjectionTime.START, None, None,
))
manager.register_script_message_handler("result")
view = WebKit2.WebView.new_with_user_content_manager(manager)
window = Gtk.Window(title="Temporary synthetic meeting-open benchmark")
window.set_accept_focus(False)
window.set_default_size(1280, 720)
window.add(view)
exit_code = 1


def report(_manager, result):
    global exit_code
    data = json.loads(result.get_js_value().to_string())
    print(json.dumps(data, indent=2), flush=True)
    exit_code = 1 if "error" in data else 0
    Gtk.main_quit()


def loaded(_view, event):
    if event != WebKit2.LoadEvent.FINISHED:
        return
    view.run_javascript("""(async () => {
      while (!document.querySelector('.ot-meeting-open'))
        await new Promise(resolve => setTimeout(resolve, 10));
      const measurements = [];
      for (const id of ['m1', 'm2', 'm3', 'm7', 'm1', 'm2', 'm3']) {
        const settle = SETTLE;
        if (settle) await new Promise(resolve => setTimeout(resolve, settle));
        const result = await window.measureMeetingOpen(id);
        if (measurements.length && result.routeRequests)
          throw Error('Meeting switch refetched the static route');
        measurements.push({ id, ...result });
      }
      window.webkit.messageHandlers.result.postMessage(JSON.stringify({
        engine: 'WebKitGTK', summaryDelay: DELAY, settleMs: SETTLE, measurements
      }));
    })().catch(error => window.webkit.messageHandlers.result.postMessage(
      JSON.stringify({error: String(error)})));
    """.replace("SETTLE", str(max(0, args.settle_ms))).replace("DELAY", str(args.summary_delay)), None, None, None)


def timeout():
    print(json.dumps({"error": "Benchmark timed out"}), flush=True)
    Gtk.main_quit()
    return False


manager.connect("script-message-received::result", report)
view.connect("load-changed", loaded)
window.connect("delete-event", lambda *_: (Gtk.main_quit(), False)[1])
window.show_all()
view.load_uri(args.base)
GLib.timeout_add_seconds(30, timeout)
Gtk.main()
window.destroy()
raise SystemExit(exit_code)
