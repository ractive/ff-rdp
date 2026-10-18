// `screenshot --viewport`: insert the WxH iframe wrapper on the live tab.
// Placeholders (substituted by crates/ff-rdp-cli/src/commands/screenshot_viewport.rs):
//   __FFRDP_W__, __FFRDP_H__  CSS px; __FFRDP_ID__  JSON string literal.
// Returns JSON {left, top, window_clipped}: the wrapper's document origin and
// whether the live window is smaller than the frame.
(function() {
  var W = __FFRDP_W__, H = __FFRDP_H__, ID = __FFRDP_ID__;
  document.querySelectorAll('[data-ffrdp-viewport]').forEach(function(e) {
    if (e.__ffrdpOnViolation) document.removeEventListener('securitypolicyviolation', e.__ffrdpOnViolation);
    e.remove();
  });
  var st = { phase: 'loading', gen: 0, images_settled: false, pending: null, probed: false,
    xfo: null, csp: null, violation: null };
  var refuse = function() {
    if (st.phase === 'refused') return;
    st.phase = 'refused';
    fetch(location.href, { credentials: 'include', cache: 'no-store' }).then(function(r) {
      st.xfo = r.headers.get('x-frame-options');
      st.csp = r.headers.get('content-security-policy');
      st.probed = true;
    }, function() { st.probed = true; });
  };
  // The page's own CSP (frame-src/child-src/default-src) can block the frame
  // before it loads; no `load` fires then, only this event.
  var onViolation = function(e) {
    var dir = e.effectiveDirective || e.violatedDirective || '';
    if (/^(frame-src|child-src|default-src)/.test(dir)
        && String(e.blockedURI || '').indexOf(location.origin) === 0) {
      st.violation = dir;
      refuse();
    }
  };
  document.addEventListener('securitypolicyviolation', onViolation);
  var d = document.createElement('div');
  d.setAttribute('data-ffrdp-viewport', ID);
  d.style.cssText = 'all: initial; display: block; position: absolute; margin: 0; padding: 0; border: 0;'
    + ' overflow: hidden; z-index: 2147483647; background: #fff;'
    + ' left: ' + window.scrollX + 'px; top: ' + window.scrollY + 'px;'
    + ' width: ' + W + 'px; height: ' + H + 'px;';
  d.__ffrdp = st;
  d.__ffrdpOnViolation = onViolation;
  // The sandbox grants neither top navigation (a frame-buster cannot navigate
  // the live tab) nor popups (the reload cannot open tabs).
  var f = document.createElement('iframe');
  f.setAttribute('sandbox', 'allow-scripts allow-same-origin allow-forms');
  f.style.cssText = 'all: initial; display: block; margin: 0; padding: 0; border: 0;'
    + ' width: ' + W + 'px; height: ' + H + 'px;';
  f.addEventListener('load', function() {
    // Every (re)load of the frame starts a new generation; callbacks of an
    // older one must not touch the state.
    var gen = ++st.gen;
    st.images_settled = false;
    var doc = null, href = null;
    try { doc = f.contentDocument; href = f.contentWindow.location.href; } catch (e) {}
    if (!doc || !href || href === 'about:blank') { refuse(); return; }
    st.phase = 'loaded';
    var imgs = Array.prototype.filter.call(doc.images, function(img) {
      var r = img.getBoundingClientRect();
      return r.bottom > 0 && r.top < H && r.right > 0 && r.left < W;
    });
    imgs.forEach(function(img) { if (img.loading === 'lazy') img.loading = 'eager'; });
    st.pending = imgs.length;
    var waits = imgs.map(function(img) {
      return new Promise(function(res) {
        if (img.complete) return res();
        img.addEventListener('load', res, { once: true });
        img.addEventListener('error', res, { once: true });
      }).then(function() {
        if (gen === st.gen) st.pending--;
        return img.decode ? img.decode().catch(function() {}) : null;
      });
    });
    if (doc.fonts && doc.fonts.ready) waits.push(doc.fonts.ready.catch(function() {}));
    Promise.all(waits).then(function() {
      var w = f.contentWindow;
      w.requestAnimationFrame(function() {
        w.requestAnimationFrame(function() { if (gen === st.gen) st.images_settled = true; });
      });
    });
  });
  f.src = location.href;
  d.appendChild(f);
  document.documentElement.appendChild(d);
  // Top layer: above an open modal <dialog>, popover or its ::backdrop.
  if (typeof d.showPopover === 'function') {
    d.setAttribute('popover', 'manual');
    try { d.showPopover(); } catch (e) {}
  }
  var r = d.getBoundingClientRect();
  return JSON.stringify({ left: r.left + window.scrollX, top: r.top + window.scrollY,
    window_clipped: W > window.innerWidth || H > window.innerHeight });
})()
