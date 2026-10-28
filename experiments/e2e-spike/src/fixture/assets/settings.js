// Impostazioni: il DOM viene costruito dopo GET /api/me e cambia a ogni azione.
const root = document.getElementById('settings-root');
const savedMessage = root.dataset.savedMessage;
const signedInAs = document.getElementById('signed-in-as');

function element(tag, text, attrs) {
  const node = document.createElement(tag);
  if (text) node.textContent = text;
  for (const [key, value] of Object.entries(attrs || {})) node.setAttribute(key, value);
  return node;
}

function renderView(displayName, status) {
  root.replaceChildren(element('p', 'Display name: ' + displayName, { id: 'display-name-line' }));
  if (status) root.append(element('p', status, { role: 'status' }));
  const edit = element('button', 'Edit display name', { type: 'button' });
  edit.addEventListener('click', () => renderEditor(displayName));
  root.append(edit);
}

function renderEditor(displayName) {
  const label = element('label', 'Display name', { for: 'display-name-input' });
  const input = element('input', '', { id: 'display-name-input', type: 'text' });
  input.value = displayName;
  const error = element('p', '', { role: 'alert', hidden: '' });
  const save = element('button', 'Save', { type: 'button' });
  const cancel = element('button', 'Cancel', { type: 'button' });
  cancel.addEventListener('click', () => renderView(displayName, ''));
  save.addEventListener('click', async () => {
    const result = await postJson('/api/settings', { display_name: input.value });
    if (result.ok) {
      signedInAs.textContent = 'Signed in as ' + result.data.display_name;
      renderView(result.data.display_name, savedMessage);
    } else {
      setAlert(error, result.data.error || 'Could not save.');
    }
  });
  root.replaceChildren(label, input, error, save, cancel);
}

(async () => {
  const response = await fetch('/api/me');
  if (!response.ok) {
    window.location.assign('/login');
    return;
  }
  const me = await response.json();
  renderView(me.display_name, '');
})();
