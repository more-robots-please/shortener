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

    const qrCheck = document.getElementById('gen-qr');
    if (qrCheck && qrCheck.checked) {
      await generateQr(shortUrl);
    } else {
      document.getElementById('qr-result').innerHTML = '';
    }
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

async function generateQr(shortUrl) {
  const qrResult = document.getElementById('qr-result');
  qrResult.innerHTML = '<span style="color:#555;font-size:0.85rem">> generating qr...</span>';

  try {
    const res = await fetch('https://qr.seraph.ws/api/generate', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ url: shortUrl, logo: true, print_mode: false })
    });
    if (!res.ok) throw new Error();
    const data = await res.json();
    const blob = new Blob([data.svg], { type: 'image/svg+xml' });
    const objUrl = URL.createObjectURL(blob);
    qrResult.innerHTML = `
      <img src="${objUrl}" alt="QR code" style="width:180px;height:180px;display:block;margin-top:1rem;border:1px solid #ff2d78" />
      <a href="https://qr.seraph.ws/?url=${encodeURIComponent(shortUrl)}" target="_blank" style="font-size:0.8rem;color:#555;margin-top:0.4rem;display:block">open in qr editor →</a>
    `;
  } catch {
    qrResult.innerHTML = '<span style="color:#ff6b6b;font-size:0.85rem">> failed to generate qr</span>';
  }
}