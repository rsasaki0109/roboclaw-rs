document.querySelectorAll("[data-copy]").forEach((button) => {
  button.addEventListener("click", async () => {
    const original = button.textContent;
    const command = button.getAttribute("data-copy") || "";
    try {
      await navigator.clipboard.writeText(command);
      button.textContent = "Copied";
    } catch (_) {
      button.textContent = "Copy failed";
    }
    window.setTimeout(() => {
      button.textContent = original;
    }, 1400);
  });
});
