import './app.scss'

document.querySelectorAll('[data-use-environment]').forEach((e) => {
    const radio = e.querySelector('input[type=radio]');
    const inputToActivate = e.querySelector('input[type=text]');
    const inputToDeactivate = document.querySelector(`[data-use-environment=${e.dataset.useEnvironment === 'true' ? 'false' : 'true'}] input[type=text]`);

    if (radio.checked) {
        inputToDeactivate.classList.add('inactive');
    }

    radio.addEventListener('click', () => {
        inputToActivate.classList.remove('inactive');
        inputToDeactivate.classList.add('inactive');
    });
});

window.addEventListener("pageshow", () => {
    document.querySelector('[data-use-environment] input[type=radio]:checked').click();
});
