// `screenshot --viewport`: read the injected frame's state as JSON.
// Placeholder: __FFRDP_ID__  JSON string literal of the wrapper id.
(function() {
  var ID = __FFRDP_ID__;
  var d = null;
  document.querySelectorAll('[data-ffrdp-viewport]').forEach(function(e) {
    if (e.getAttribute('data-ffrdp-viewport') === ID) d = e;
  });
  var st = d && d.__ffrdp;
  if (!st) return JSON.stringify({ phase: 'gone' });
  var f = d.querySelector('iframe'), ready = null, href = null;
  try { ready = f.contentDocument && f.contentDocument.readyState; href = f.contentWindow.location.href; } catch (e) {}
  return JSON.stringify({ phase: st.phase, images_settled: st.images_settled, pending: st.pending,
    probed: st.probed, xfo: st.xfo, csp: st.csp, violation: st.violation, ready: ready, href: href });
})()
