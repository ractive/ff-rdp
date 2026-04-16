// Issue #13: ReferenceError — nonExistentFunction is not defined
window.addEventListener('load', function () {
  nonExistentFunction();
});

// Issue #26: Late-injected banner causing CLS after 2 seconds
setTimeout(function () {
  var banner = document.createElement('div');
  banner.id = 'promo-banner';
  banner.style.cssText =
    'background:#e74c3c;color:#fff;padding:20px;text-align:center;' +
    'font-size:18px;position:relative;z-index:1000;';
  banner.innerHTML =
    '<strong>Sonderaktion!</strong> Jetzt besichtigen und 1 Monat gratis erhalten!';
  var body = document.body;
  if (body.firstChild) {
    body.insertBefore(banner, body.firstChild);
  } else {
    body.appendChild(banner);
  }
}, 2000);
