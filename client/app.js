import './app.scss';

const environmentElement = document.getElementById('environment');
const targetElement = document.getElementById('target');

const fillTarget = () => {
    const environment = environmentElement.value.trim();

    targetElement.value = environment
        ? `https://${environment}-ats.mgspdtesting.com/${environment}/home/saml.hms`
        : 'http://localhost:8080/combined-app/home/saml.hms';
}

environmentElement.addEventListener('change', fillTarget);
environmentElement.addEventListener('keyup', fillTarget);
