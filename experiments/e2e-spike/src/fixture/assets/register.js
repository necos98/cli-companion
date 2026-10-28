// Registrazione: validazione lato client, poi POST /api/register.
const form = document.getElementById('register-form');
const fields = {
  name: document.getElementById('name'),
  email: document.getElementById('email'),
  password: document.getElementById('password'),
  confirm: document.getElementById('confirm-password'),
  terms: document.getElementById('terms'),
};
const errors = {
  name: document.getElementById('name-error'),
  email: document.getElementById('email-error'),
  password: document.getElementById('password-error'),
  confirm: document.getElementById('confirm-error'),
  terms: document.getElementById('terms-error'),
};
const formError = document.getElementById('form-error');

function validate() {
  const found = {};
  if (fields.name.value.trim() === '') found.name = 'Please enter your full name.';
  if (!/^[^@\s]+@[^@\s.][^@\s]*\.[^@\s.]+$/.test(fields.email.value.trim())) {
    found.email = 'Please enter a valid email address.';
  }
  if (fields.password.value.length < 8) found.password = 'Password must be at least 8 characters.';
  if (fields.password.value !== fields.confirm.value) found.confirm = 'Passwords do not match.';
  if (!fields.terms.checked) found.terms = 'You must accept the terms.';
  return found;
}

form.addEventListener('submit', async (event) => {
  event.preventDefault();
  setAlert(formError, '');
  const found = validate();
  for (const key of Object.keys(errors)) setAlert(errors[key], found[key]);
  if (Object.keys(found).length > 0) return;
  const result = await postJson('/api/register', {
    name: fields.name.value,
    email: fields.email.value,
    password: fields.password.value,
    confirm_password: fields.confirm.value,
    accept_terms: fields.terms.checked,
  });
  if (result.ok) {
    window.location.assign('/welcome');
  } else {
    setAlert(formError, result.data.error || 'Registration failed.');
  }
});
