import './app.scss'

document.querySelectorAll('[data-use-environment]').forEach((e) => {
    const radio = e.querySelector('input[type=radio]');
    const inputToActivate = e.querySelector('input[type=text]');
    const inputToDeactivate = document.querySelector(`[data-use-environment=${e.dataset.useEnvironment === 'true' ? 'false' : 'true'}] input[type=text]`);

    if (radio.checked) {
        inputToDeactivate.classList.add('opacity-50');
    }

    radio.addEventListener('click', () => {
        inputToActivate.classList.remove('opacity-50');
        inputToDeactivate.classList.add('opacity-50');
    });
});

window.addEventListener("pageshow", () => {
    document.querySelector('[data-use-environment] input[type=radio]:checked').click();
});
