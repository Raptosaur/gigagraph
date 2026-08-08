const queue = [];

export function trackEvent(name, payload) {
  queue.push({ name, payload, at: Date.now() });
  if (queue.length > 10) flush();
}

export function flush() {
  const batch = queue.splice(0, queue.length);
  return fetch("/api/events", { method: "POST", body: JSON.stringify(batch) });
}

export const identify = function (userId) {
  return trackEvent("identify", { userId });
};

module.exports = { trackEvent, flush, identify };
