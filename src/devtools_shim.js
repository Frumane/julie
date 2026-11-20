// Gömülü geliştirici araçlarının sayfayla konuşma yolu. Araçlar normalde
// sayfaya WebSocket ile bağlanır; burada o bağlantı Julie'ye gider
// (`window.ipc`), Julie de komutu sekmenin motoruna iletir. Yanıtlar ve
// olaylar `chrome.webview` mesajı olarak geri gelir.
(() => {
  if (location.host !== "julie.devtools") return;
  const send = (text) => window.ipc.postMessage(text);

  // İlk açılışta araçlar sistemin dilinde (Türkçe) gelsin; kullanıcı
  // ayarlardan değiştirirse onun seçimi kalır.
  try {
    if (localStorage.getItem("language") === null) {
      localStorage.setItem("language", '"browserLanguage"');
    }
  } catch {}
  const Native = window.WebSocket;
  let socket = null;

  class JulieSocket extends EventTarget {
    constructor(url) {
      super();
      this.url = String(url);
      this.protocol = "";
      this.extensions = "";
      this.binaryType = "blob";
      this.bufferedAmount = 0;
      this.readyState = 0;
      this.onopen = this.onmessage = this.onclose = this.onerror = null;
      socket = this;
      setTimeout(() => {
        this.readyState = 1;
        this.fire(new Event("open"));
      }, 0);
    }
    fire(event) {
      this.dispatchEvent(event);
      const handler = this["on" + event.type];
      if (typeof handler === "function") handler.call(this, event);
    }
    send(data) {
      if (this.readyState === 1) send(String(data));
    }
    close() {
      if (this.readyState >= 2) return;
      this.readyState = 3;
      if (socket === this) socket = null;
      this.fire(new CloseEvent("close", { wasClean: true, code: 1000 }));
    }
  }
  JulieSocket.CONNECTING = 0;
  JulieSocket.OPEN = 1;
  JulieSocket.CLOSING = 2;
  JulieSocket.CLOSED = 3;

  window.WebSocket = function (url, protocols) {
    return String(url).startsWith("ws://julie-devtools/")
      ? new JulieSocket(url)
      : new Native(url, protocols);
  };
  Object.assign(window.WebSocket, JulieSocket);
  window.WebSocket.prototype = Native.prototype;

  window.chrome.webview.addEventListener("message", (event) => {
    if (socket) socket.fire(new MessageEvent("message", { data: event.data }));
  });

  // Araçların "kapat" komutu Julie'ye gitsin.
  let host;
  Object.defineProperty(globalThis, "InspectorFrontendHost", {
    configurable: true,
    get: () => host,
    set: (value) => {
      host = value;
      if (value) value.closeWindow = () => send("julie:close");
    },
  });
})();
