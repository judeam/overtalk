(() => {
  const api = globalThis.browser || chrome;
  function report() {
    if (document.visibilityState !== "visible") return;
    const active = [...document.querySelectorAll("video")].some(video => !video.paused && video.readyState >= 2);
    api.runtime.sendMessage({ type: "meeting", title: document.title, active }).catch(() => {});
  }
  report();
  setInterval(report, 15000);
  document.addEventListener("visibilitychange", report);
})();
