/* No analytics, remote requests, page text scraping, or audio/video access. */
const api = globalThis.browser || chrome;
const action = api.action || api.browserAction;
let enabled = false;
let port;
api.storage.local.get("enabled").then(value => { enabled = value.enabled === true; });
action.onClicked.addListener(async () => {
  enabled = !enabled;
  await api.storage.local.set({ enabled });
  action.setBadgeText({ text: enabled ? "ON" : "" });
  if (!enabled) { port?.disconnect(); port = undefined; }
});
api.runtime.onMessage.addListener((message, sender) => {
  if (!enabled || message.type !== "meeting" || !sender.tab || sender.frameId !== 0) return;
  const source = new URL(sender.url || "https://invalid.invalid");
  if (source.protocol !== "https:" || !/^(meet\.google\.com|([a-z0-9-]+\.)?zoom\.us|teams\.microsoft\.com|teams\.live\.com|[a-z0-9-]+\.webex\.com)$/.test(source.hostname)) return;
  if (!port) {
    port = api.runtime.connectNative("com.overtalk.meetings");
    port.onDisconnect.addListener(() => { port = undefined; });
  }
  port.postMessage({ url: source.href, title: String(message.title || "Meeting page").slice(0, 240), active: message.active === true });
});
