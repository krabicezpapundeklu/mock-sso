import './app.scss'

// post values of all inputs, including disabled ones
document.getElementById('submit_form').addEventListener('formdata', (e) => {
    const formData = e.formData;

    for (const input of document.querySelectorAll('input:not([type=radio]), input:checked')) {
        formData.set(input.name, input.value);
    }
});

document.querySelectorAll('[data-use-environment]').forEach((e) => {
    const radio = e.querySelector('input[type=radio]');
    const inputToEnable = e.querySelector('input[type=text]');
    const inputToDisable = document.querySelector(`[data-use-environment=${e.dataset.useEnvironment === 'true' ? 'false' : 'true'}] input[type=text]`);

    if (radio.checked) {
        inputToDisable.disabled = true;
    }

    radio.addEventListener('click', () => {
        inputToEnable.disabled = false;
        inputToDisable.disabled = true;
    });
});
