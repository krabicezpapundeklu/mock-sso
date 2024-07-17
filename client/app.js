import './app.scss'

const environmentElement = document.getElementById('environment');
const customTargetElement = document.getElementById('custom_target');
const useEnvironmentElement = document.getElementById('use_environment');
const useCustomTargetElement = document.getElementById('use_custom_target');

const useEnvironment = (use) => {
    if (use) {
        environmentElement.disabled = false;
        useEnvironmentElement.checked = true;
        customTargetElement.disabled = true;
    } else {
        customTargetElement.disabled = false;
        useCustomTargetElement.checked = true;
        environmentElement.disabled = true;
    }
}

useEnvironmentElement.addEventListener('click', () => useEnvironment(true));
useCustomTargetElement.addEventListener('click', () => useEnvironment(false));

window.addEventListener("pageshow", () => {
    useEnvironment(useEnvironmentElement.checked);
});
