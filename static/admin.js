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
  loadAnalytics();
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


async function loadAnalytics() {
  const res = await fetch('/admin/analytics', {
    headers: { 'Authorization': 'Bearer ' + TOKEN }
  });
  if (!res.ok) return;
  const d = await res.json();

  // Summary cards
  document.getElementById('stat-total-links').textContent = d.total_links;
  document.getElementById('stat-total-clicks').textContent = d.total_clicks;
  document.getElementById('stat-active').textContent = d.active_links;
  document.getElementById('stat-expired').textContent = d.expired_links;

  // Clicks per day chart
  const ctx = document.getElementById('clicks-chart').getContext('2d');
  new Chart(ctx, {
    type: 'bar',
    data: {
      labels: d.clicks_per_day.map(r => r.day),
      datasets: [{
        label: 'clicks',
        data: d.clicks_per_day.map(r => r.clicks),
        backgroundColor: '#ff2d78aa',
        borderColor: '#ff2d78',
        borderWidth: 1,
      }]
    },
    options: {
      responsive: true,
      maintainAspectRatio: false,
      plugins: { legend: { display: false } },
      scales: {
        x: { ticks: { color: '#888', font: { family: 'Hack, monospace', size: 10 } }, grid: { color: '#1a1a1a' } },
        y: { ticks: { color: '#888', font: { family: 'Hack, monospace', size: 10 } }, grid: { color: '#1a1a1a' }, beginAtZero: true }
      }
    }
  });

  // Breakdown lists
  function renderBreakdown(id, rows) {
    const el = document.getElementById(id);
    el.innerHTML = rows.length
      ? rows.map(r => `<div class="breakdown-row"><span>${r.label}</span><span class="count">${r.count}</span></div>`).join('')
      : '<div class="breakdown-row"><span style="color:#555">no data yet</span></div>';
  }
  renderBreakdown('breakdown-countries', d.top_countries);
  renderBreakdown('breakdown-browsers', d.top_browsers);
  renderBreakdown('breakdown-os', d.top_os);

  // Top links
  const topTable = document.getElementById('top-links-table');
  topTable.innerHTML = `<tr><th>code</th><th>url</th><th>clicks</th></tr>`;
  for (const l of d.top_links) {
    topTable.innerHTML += `<tr>
      <td><a href="/${l.code}">${l.code}</a></td>
      <td><a href="${l.url}" target="_blank">${l.url.substring(0, 60)}${l.url.length > 60 ? '...' : ''}</a></td>
      <td>${l.clicks}</td>
    </tr>`;
  }
}