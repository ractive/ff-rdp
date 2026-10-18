// `screenshot --viewport`: remove every wrapper (also leftovers of an
// interrupted run) and its CSP listener. Returns how many were removed.
(function() {
  var n = 0;
  document.querySelectorAll('[data-ffrdp-viewport]').forEach(function(e) {
    if (e.__ffrdpOnViolation) document.removeEventListener('securitypolicyviolation', e.__ffrdpOnViolation);
    e.remove();
    n++;
  });
  return n;
})()
