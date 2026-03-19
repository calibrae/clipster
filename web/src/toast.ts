let toastArea: HTMLElement | null = null;

export function initToast(el: HTMLElement): void {
  toastArea = el;
}

export function showToast(message: string): void {
  if (!toastArea) return;
  const toast = document.createElement('div');
  toast.className = 'toast';
  toast.textContent = message;
  toastArea.appendChild(toast);
  setTimeout(() => {
    toast.classList.add('toast-out');
    toast.addEventListener('animationend', () => toast.remove());
  }, 2000);
}
