async function shorten() {
  const url = document.getElementById('url').value.trim();
  const code = document.getElementById('code').value.trim();
  const result = document.getElementById('result');
  if (!url) { result.textContent = 'please enter a url'; result.className = 'error'; return; }

  const expiresVal = parseInt(document.getElementById('expires-value').value);
  const expiresUnit = parseInt(document.getElementById('expires-unit').value);
  const expiresMinutes = (expiresVal && expiresVal >= 1)
    ? Math.min(expiresVal * expiresUnit, 10080)
    : null;

  const maxClicks = document.getElementById('max-clicks').value
    ? parseInt(document.getElementById('max-clicks').value)
    : null;

  const res = await fetch('/api/shorten', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      url,
      code: code || null,
      expires_in_minutes: expiresMinutes,
      max_clicks: maxClicks,
      password: document.getElementById('password').value || null
    })
  });

  const text = await res.text();
  if (res.ok) {
    const data = JSON.parse(text);
    const shortUrl = window.BASE_URL + '/' + data.code;
    result.innerHTML = '> <a href="/' + data.code + '">' + shortUrl + '</a> <button onclick="copy(this, \'' + shortUrl + '\')" style="padding:2px 8px;font-size:0.8rem">copy</button>';
    result.className = '';
  } else {
    result.textContent = '> error: ' + text;
    result.className = 'error';
  }
}

async function copy(btn, text) {
  await navigator.clipboard.writeText(text);
  btn.textContent = 'copied!';
  setTimeout(() => btn.textContent = 'copy', 1500);
}

document.addEventListener('keydown', e => { if (e.key === 'Enter') shorten(); });
