// Login: POST /api/login, poi dashboard.
const loginForm = document.getElementById('login-form');
const loginError = document.getElementById('form-error');

loginForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  setAlert(loginError, '');
  const result = await postJson('/api/login', {
    email: document.getElementById('email').value,
    password: document.getElementById('password').value,
  });
  if (result.ok) {
    window.location.assign('/app');
  } else {
    setAlert(loginError, result.data.error || 'Sign in failed.');
  }
});
