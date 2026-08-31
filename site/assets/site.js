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

  const RELEASE_API =
    "https://api.github.com/repos/SeasonXue/mac-never-sleep/releases/latest";
  const CACHE_KEY = "never-sleep-latest-tag";

  // GitHub Release tags are the workflow's tag space, not just dotted versions:
  // v0.1.1, release/v1, and release#1 must all display as returned.
  function latestReleaseLabel(tagName) {
    if (typeof tagName !== "string") return null;
    const tag = tagName.trim();
    return tag === "" ? null : tag;
  }

  function applyReleaseTag(tagName) {
    const label = latestReleaseLabel(tagName);
    if (!label) return;
    document.querySelectorAll("[data-release-tag]").forEach((el) => {
      el.textContent = label;
    });
  }

  function refreshLatestRelease() {
    try {
      const cached = sessionStorage.getItem(CACHE_KEY);
      if (cached) applyReleaseTag(cached);
    } catch {
      /* private mode: keep the HTML fallback */
    }
    fetch(RELEASE_API, { headers: { Accept: "application/vnd.github+json" } })
      .then((res) => (res.ok ? res.json() : null))
      .then((data) => {
        if (!data || !data.tag_name) return;
        const label = latestReleaseLabel(data.tag_name);
        if (!label) return;
        try {
          sessionStorage.setItem(CACHE_KEY, label);
        } catch {
          /* ignore quota */
        }
        applyReleaseTag(label);
      })
      .catch(() => {
        /* keep the Info.plist fallback printed in the HTML */
      });
  }

  if (document.querySelector("[data-release-tag]")) {
    refreshLatestRelease();
  }
})();
