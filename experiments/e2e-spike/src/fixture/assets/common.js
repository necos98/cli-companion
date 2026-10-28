// Utilita' condivise dalle pagine della fixture.
async function postJson(url, payload) {
  const response = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  let data = {};
  try {
    data = await response.json();
  } catch (e) {
    data = {};
  }
  return { ok: response.ok, status: response.status, data };
}

function setAlert(element, message) {
  element.textContent = message || '';
  element.hidden = !message;
}

const signOut = document.getElementById('signout');
if (signOut) {
  signOut.addEventListener('click', async () => {
    await postJson('/api/logout', {});
    window.location.assign('/login');
  });
}
