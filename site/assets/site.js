(() => {
  const copied = {
    en: "Copied",
    zh: "已复制",
  };
  const lang = document.documentElement.lang.startsWith("zh") ? "zh" : "en";
  document.querySelectorAll("[data-copy]").forEach((btn) => {
    const original = btn.innerHTML;
    btn.addEventListener("click", async () => {
      const text = btn.getAttribute("data-copy") || "";
      try {
        await navigator.clipboard.writeText(text);
        btn.textContent = copied[lang];
        window.setTimeout(() => {
          btn.innerHTML = original;
        }, 1600);
      } catch {
        /* clipboard may be blocked; the command is still visible in the page */
      }
    });
  });
})();
