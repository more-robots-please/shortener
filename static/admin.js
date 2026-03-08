let TOKEN = localStorage.getItem('admin_token');

if (!TOKEN) {
  document.body.innerHTML = `
    <div class="login">
      <h1>seraph / admin</h1>
      <input id="token-input" type="password" placeholder="admin token" autocomplete="off" />
      <button onclick="login()">enter</button>
    </div>`;
} else {
  loadLinks();
}

function login() {
  TOKEN = document.getElementById('token-input').value;
  localStorage.setItem('admin_token', TOKEN);
  location.reload();
}

async function loadLinks() {
  const res = await fetch('/admin/links', {
    headers: { 'Authorization': 'Bearer ' + TOKEN }
  });
  if (res.status === 401) {
    localStorage.removeItem('admin_token');
    location.reload();
    return;
  }
  const links = await res.json();
  const table = document.getElementById('links-table');
  table.innerHTML = `<tr>
    <th>code</th><th>url</th><th>clicks</th><th>created</th><th>expires</th><th>max clicks</th><th>pw</th><th></th>
  </tr>`;
  for (const l of links) {
    const expired = (l.expires_at && new Date(l.expires_at) < new Date())
      || (l.max_clicks && l.clicks >= l.max_clicks);
    const expiresAt = l.expires_at
      ? new Date(l.expires_at).toLocaleString()
      : '—';
    const maxClicks = l.max_clicks ?? '—';
    table.innerHTML += `<tr style="${expired ? 'opacity:0.45' : ''}">
      <td><a href="/${l.code}">${l.code}</a>${expired ? ' <span class="expired-badge">[expired]</span>' : ''}</td>
      <td><a href="${l.url}" target="_blank">${l.url.substring(0, 50)}${l.url.length > 50 ? '...' : ''}</a></td>
      <td>${l.clicks}</td>
      <td>${new Date(l.created_at).toLocaleDateString()}</td>
      <td>${expiresAt}</td>
      <td>${maxClicks}</td>
      <td>${l.password_hash ? '🔒' : '—'}</td>
      <td><button onclick="del(${l.id})">delete</button></td>
    </tr>`;
  }
}

async function shorten() {
  const url = document.getElementById('url').value;
  const code = document.getElementById('code').value;
  const res = await fetch('/api/shorten', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', 'Authorization': 'Bearer ' + TOKEN },
    body: JSON.stringify({ url, code: code || null })
  });
  const data = await res.json();
  document.getElementById('result').textContent = res.ok
    ? 'shortened: ' + data.short_url
    : 'error: ' + JSON.stringify(data);
  if (res.ok) loadLinks();
}

async function del(id) {
  await fetch('/admin/links/' + id, {
    method: 'DELETE',
    headers: { 'Authorization': 'Bearer ' + TOKEN }
  });
  loadLinks();
}
