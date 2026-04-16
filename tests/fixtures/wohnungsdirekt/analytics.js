// Issue #24: Cookie without Secure flag
document.cookie = 'wd_session=abc123; path=/; SameSite=Lax';

// Issue #25: Cookie without HttpOnly flag
// Note: HttpOnly cannot be set via JavaScript at all — this is inherent.
// The cookie below is missing both Secure and HttpOnly.
// SameSite=Lax (not None) so modern browsers accept the cookie without Secure.
document.cookie = 'wd_tracking=xyz789; path=/; SameSite=Lax';

// Issue #31: Mixed content — HTTP image on potentially HTTPS page
(function () {
  var pixel = new Image();
  pixel.src = 'http://example.com/track.gif?r=' + Math.random();
  pixel.style.display = 'none';
  document.body.appendChild(pixel);
})();
