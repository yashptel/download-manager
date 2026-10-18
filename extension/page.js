// OpenDM Connect, page side. Runs in the page's own JavaScript world so it
// sees what the page's scripts load: playlists fetched by script, playlist
// URLs inside JSON answers, and playlists a player builds in memory and plays
// from a blob: URL. Finds go to content.js as "opendm-media" events.
"use strict";

(() => {
  const MAX_TEXT = 2 * 1024 * 1024;
  const MAX_FINDS_PER_TEXT = 20;
  const PLAYLIST_URL = /\.(m3u8|mpd)(?:$|[?#])/i;
  // Playlist URLs written into JSON or script, after unescaping "\/".
  const URL_IN_TEXT = /https?:\/\/[^\s"'<>\\]+?\.(?:m3u8|mpd)(?:\?[^\s"'<>\\]*)?/gi;
  const READABLE_TYPE = /json|javascript|text\/plain|xml|mpegurl|dash/i;
  const reported = new Map(); // url → true once its content was sent

  function emit(found) {
    document.dispatchEvent(new CustomEvent("opendm-media", { detail: JSON.stringify(found) }));
  }

  function report(url, kind, content) {
    if (reported.get(url) === true || (reported.has(url) && !content)) return;
    reported.set(url, Boolean(content));
    // A media playlist lists segments; a master playlist lists qualities.
    const variant = kind === "hls" && content ? !content.includes("#EXT-X-STREAM-INF") : undefined;
    emit({ url, kind, content: content || undefined, variant });
  }

  function playlistKind(url, type, text) {
    if (text) {
      const head = text.slice(0, 1024).trimStart();
      if (head.startsWith("#EXTM3U")) return "hls";
      if (/<MPD[\s>]/.test(text.slice(0, 4096))) return "dash";
      return "";
    }
    if (/mpegurl/i.test(type)) return "hls";
    if (/dash\+xml/i.test(type)) return "dash";
    const match = PLAYLIST_URL.exec(url);
    return match ? (match[1].toLowerCase() === "mpd" ? "dash" : "hls") : "";
  }

  /** A response body or blob the page received. */
  function inspectText(url, type, text) {
    if (!text) return;
    const kind = playlistKind(url, type, text);
    if (kind) return report(url, kind, text.length <= MAX_TEXT ? text : undefined);
    if (url.startsWith("blob:") || !/json|javascript|text\/plain|xml|html/i.test(type)) return;
    const plain = text.replace(/\\\//g, "/").replace(/\\u0026/gi, "&");
    let count = 0;
    for (const [found] of plain.matchAll(URL_IN_TEXT)) {
      report(found, /\.mpd/i.test(found) ? "dash" : "hls");
      if (++count >= MAX_FINDS_PER_TEXT) break;
    }
  }

  function worthReading(url, type, length) {
    if (length > MAX_TEXT || /event-stream/i.test(type)) return false;
    return PLAYLIST_URL.test(url) || READABLE_TYPE.test(type);
  }

  /** Reads at most MAX_TEXT of a copy of the response; the page keeps its own. */
  async function readCopy(response) {
    const reader = response.clone().body?.getReader();
    if (!reader) return "";
    const decoder = new TextDecoder();
    let text = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return text + decoder.decode();
      text += decoder.decode(value, { stream: true });
      if (text.length > MAX_TEXT) {
        reader.cancel().catch(() => {});
        return "";
      }
    }
  }

  // fetch: the copy is taken before the page's own callbacks run.
  const nativeFetch = window.fetch;
  window.fetch = function fetch(...args) {
    const answer = nativeFetch.apply(this, args);
    answer.then((response) => {
      try {
        if (!response.ok || !response.url) return;
        const type = response.headers.get("content-type") || "";
        const length = Number(response.headers.get("content-length")) || 0;
        if (!worthReading(response.url, type, length)) return;
        readCopy(response).then((text) => inspectText(response.url, type, text), () => {});
      } catch {}
    }, () => {});
    return answer;
  };

  // XMLHttpRequest: read the answer once it has loaded.
  function onXhrLoad() {
    try {
      const url = this.responseURL;
      const type = this.getResponseHeader("content-type") || "";
      if (!url || this.status >= 400 || !worthReading(url, type, 0)) return;
      let text = "";
      if (this.responseType === "" || this.responseType === "text") text = this.responseText;
      else if (this.responseType === "json" && this.response) text = JSON.stringify(this.response);
      else if (this.responseType === "arraybuffer" && this.response?.byteLength <= MAX_TEXT) text = new TextDecoder().decode(this.response);
      if (text.length <= MAX_TEXT) inspectText(url, type, text);
    } catch {}
  }
  const nativeSend = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.send = function send(...args) {
    this.addEventListener("load", onXhrLoad);
    return nativeSend.apply(this, args);
  };

  // Playlists built in memory: players hand them to the video as blob: URLs.
  const nativeCreateObjectURL = URL.createObjectURL;
  URL.createObjectURL = function createObjectURL(object) {
    const url = nativeCreateObjectURL.call(this, object);
    try {
      if (object instanceof Blob && object.size > 0 && object.size <= 512 * 1024 && /^$|text|mpegurl|dash|xml/i.test(object.type)) {
        object.text().then((text) => inspectText(url, object.type, text), () => {});
      }
    } catch {}
    return url;
  };
})();

