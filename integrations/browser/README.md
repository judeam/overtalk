# Local meeting detection

The extension is **off by default**. Click its toolbar icon to enable it. Only a
supported, visible meeting tab's URL, title and video-active flag go to Overtalk
through browser native messaging. It never reads messages or microphone audio,
joins a call, makes a network request or starts a recording.

Build `overtalk-browser-bridge`, register it using
`python scripts/register-browser-bridge.py --binary <path> --extension-id <id>`,
then load this directory as an unpacked Chromium extension. The browser supplies
its extension ID on the extension-management page. Firefox uses the generated
Firefox manifest and its fixed add-on ID; distribution requires Mozilla signing.
Browser installation and enablement remain deliberate user actions.
