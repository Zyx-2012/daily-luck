(() => {
  "use strict";

  const STORAGE_KEY = "jrrp-web-pcl2-identifier-v2";
  const PCLCE_STORAGE_KEY = "jrrp-web-pclce-identifier-v1";
  const PCL2_AUTO_KEY = "jrrp-web-pcl2-auto-identifier";
  const PCLCE_AUTO_KEY = "jrrp-web-pclce-auto-identifier";
  const THEME_STORAGE_KEY = "jrrp-web-theme";
  const MASK_64 = (1n << 64n) - 1n;
  const HASH_XOR = 0xA98F501BC684032Fn;
  const WEEKDAYS = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

  const $ = (selector) => document.querySelector(selector);
  const els = {
    heroDate: $("#hero-date"),
    scoreDateLabel: $("#score-date-label"),
    scoreStatus: $("#score-status"),
    scoreRing: $("#score-ring"),
    todayScore: $("#today-score"),
    scoreMessageTitle: $("#score-message-title"),
    scoreMessage: $("#score-message"),
    scoreScaleFill: $("#score-scale-fill"),
    startDate: $("#start-date"),
    daysCount: $("#days-count"),
    identifier: $("#identifier"),
    identifierStatus: $("#identifier-status"),
    pclceIdentifier: $("#pclce-identifier"),
    pclceIdentifierStatus: $("#pclce-identifier-status"),
    pclceTodayScore: $("#pclce-today-score"),
    pclceScoreStatus: $("#pclce-score-status"),
    forecastBody: $("#forecast-body"),
    emptyState: $("#empty-state"),
    pcl2NextPerfectDate: $("#pcl2-next-perfect-date"),
    pcl2NextPerfectCountdown: $("#pcl2-next-perfect-countdown"),
    pclceNextPerfectDate: $("#pclce-next-perfect-date"),
    pclceNextPerfectCountdown: $("#pclce-next-perfect-countdown"),
    pcl2WindowHighScore: $("#pcl2-window-high-score"),
    pcl2WindowHighDate: $("#pcl2-window-high-date"),
    pclceWindowHighScore: $("#pclce-window-high-score"),
    pclceWindowHighDate: $("#pclce-window-high-date"),
    pcl2PerfectCount: $("#pcl2-perfect-count"),
    pcl2WindowRange: $("#pcl2-window-range"),
    pcl2PerfectList: $("#pcl2-perfect-list"),
    pclcePerfectCount: $("#pclce-perfect-count"),
    pclceWindowRange: $("#pclce-window-range"),
    pclcePerfectList: $("#pclce-perfect-list")
  };

  const themeToggle = $("#theme-toggle");
  const themeToggleIcon = $("#theme-toggle-icon");
  const themeToggleLabel = $("#theme-toggle-label");
  const themeColorMeta = document.querySelector('meta[name="theme-color"]');

  function applyTheme(isDark, persist = true) {
    document.documentElement.classList.toggle("dark", isDark);
    themeToggleIcon.textContent = isDark ? "☀" : "☾";
    themeToggleLabel.textContent = isDark ? "浅色" : "深色";
    themeToggle.setAttribute("aria-label", isDark ? "切换浅色模式" : "切换深色模式");
    themeToggle.title = isDark ? "切换浅色模式" : "切换深色模式";
    if (themeColorMeta) themeColorMeta.content = isDark ? "#0f1725" : "#f4f7fb";
    if (persist) localStorage.setItem(THEME_STORAGE_KEY, isDark ? "dark" : "light");
  }

  function initTheme() {
    const savedTheme = localStorage.getItem(THEME_STORAGE_KEY);
    const prefersDark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
    applyTheme(savedTheme ? savedTheme === "dark" : prefersDark, false);
  }

  function pad(value) {
    return String(value).padStart(2, "0");
  }

  function toDateInputValue(date) {
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  function dateFromInput(value) {
    const [year, month, day] = value.split("-").map(Number);
    return new Date(year, month - 1, day, 12, 0, 0, 0);
  }

  function addDays(date, amount) {
    const result = new Date(date);
    result.setDate(result.getDate() + amount);
    return result;
  }

  function dateKey(date) {
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  }

  function formatDate(date, includeWeekday = false) {
    const base = `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()}`;
    return includeWeekday ? `${base} ${WEEKDAYS[date.getDay()]}` : base;
  }

  function dayOfYear(date) {
    const start = new Date(date.getFullYear(), 0, 1);
    const current = new Date(date.getFullYear(), date.getMonth(), date.getDate());
    return Math.floor((current - start) / 86400000) + 1;
  }

  // PCL2's stable hash is a 64-bit unsigned rolling hash. BigInt keeps the
  // intermediate values exact, which matters when reproducing the VB code.
  function stableHash(value) {
    let result = 5381n;
    for (let index = 0; index < value.length; index += 1) {
      result = (((result << 5n) ^ result ^ BigInt(value.charCodeAt(index))) & MASK_64);
    }
    return result ^ HASH_XOR;
  }

  function roundEven(value) {
    const lower = Math.floor(value);
    const fraction = value - lower;
    if (fraction < 0.5) return lower;
    if (fraction > 0.5) return lower + 1;
    return lower % 2 === 0 ? lower : lower + 1;
  }

  function scoreForDate(date, identifier) {
    const firstSeed = `asdfgbn${dayOfYear(date)}12#3$45${date.getFullYear()}IUY`;
    const secondSeed = `QWERTY${identifier}0*8&6${date.getDate()}kjhg`;
    const firstHash = Number(stableHash(firstSeed)) / 3;
    const secondHash = Number(stableHash(secondSeed)) / 3;
    const raw = Math.abs((firstHash + secondHash) / 527) % 1001;
    const rounded = roundEven(raw);
    return rounded >= 970 ? 100 : roundEven((rounded / 969) * 99);
  }

  function djb2Hash(value) {
    let hash = 5381;
    for (let index = 0; index < value.length; index += 1) {
      hash = (hash * 33 + value.charCodeAt(index)) % 0x100000000;
    }
    return hash % 0x80000000;
  }

  // PCLCE calls the explicitly seeded .NET Random implementation. Its
  // subtractive generator is kept here so the result matches Next(0, 101).
  function dotnetRandomNext101(seed) {
    const mbig = 2147483647;
    const mseed = 161803398;
    const seedArray = new Array(56).fill(0);
    let mj = mseed - Math.abs(seed);
    seedArray[55] = mj;
    let mk = 1;

    for (let index = 1; index <= 54; index += 1) {
      const ii = (21 * index) % 55;
      seedArray[ii] = mk;
      mk = mj - mk;
      if (mk < 0) mk += mbig;
      mj = seedArray[ii];
    }

    for (let pass = 1; pass <= 4; pass += 1) {
      for (let index = 1; index <= 55; index += 1) {
        let value = seedArray[index] - seedArray[1 + ((index + 30) % 55)];
        if (value < 0) value += mbig;
        seedArray[index] = value;
      }
    }

    let inext = 0;
    let inextp = 21;
    const internalSample = () => {
      let locInext = inext + 1;
      if (locInext >= 56) locInext = 1;
      let locInextp = inextp + 1;
      if (locInextp >= 56) locInextp = 1;
      let value = seedArray[locInext] - seedArray[locInextp];
      if (value === mbig) value -= 1;
      if (value < 0) value += mbig;
      seedArray[locInext] = value;
      inext = locInext;
      inextp = locInextp;
      return value;
    };

    return Math.floor((internalSample() / mbig) * 101);
  }

  function pclceScoreForDate(date, identifier) {
    const datePart = `${date.getFullYear()}${pad(date.getMonth() + 1)}${pad(date.getDate())}`;
    return dotnetRandomNext101(djb2Hash(`${datePart}${identifier}`));
  }

  function getDefaultIdentifier(storageKey) {
    let value = localStorage.getItem(storageKey);
    if (value) return value;
    const bytes = new Uint8Array(6);
    if (window.crypto && window.crypto.getRandomValues) {
      window.crypto.getRandomValues(bytes);
      value = `WEB-${Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("").toUpperCase()}`;
    } else {
      value = `WEB-${Date.now().toString(36).toUpperCase()}`;
    }
    localStorage.setItem(storageKey, value);
    return value;
  }

  function setIdentifierStatus(text, state = "") {
    els.identifierStatus.textContent = text;
    els.identifierStatus.className = `identifier-status${state ? ` ${state}` : ""}`;
  }

  function setPclceIdentifierStatus(text, state = "") {
    els.pclceIdentifierStatus.textContent = text;
    els.pclceIdentifierStatus.className = `identifier-status${state ? ` ${state}` : ""}`;
  }

  async function loadIdentifiers() {
    setIdentifierStatus("读取本机 PCL2 中");
    setPclceIdentifierStatus("读取本机 PCLCE 中");
    try {
      const response = await fetch("/api/identifiers", { cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const data = await response.json();
      const pcl = data.pcl;
      const pclce = data.pclce;
      if (!pcl || !pcl.available || !pcl.identify) {
        setIdentifierStatus("未找到 PCL2");
      } else {
        const current = els.identifier.value.trim();
        const previousAuto = localStorage.getItem(PCL2_AUTO_KEY);
        // Migrate the numeric registry seed used by the earlier local backend.
        const isLegacyPclSeed = /^\d{10,}$/.test(current);
        const canUsePclValue = !current || current.startsWith("WEB-") || isLegacyPclSeed || current === previousAuto;
        if (canUsePclValue) {
          els.identifier.value = pcl.identify;
          localStorage.setItem(STORAGE_KEY, pcl.identify);
          localStorage.setItem(PCL2_AUTO_KEY, pcl.identify);
          setIdentifierStatus("已读取 PCL2", "is-connected");
        } else {
          setIdentifierStatus("PCL2 可用 · 保留手动值", "is-connected");
        }
      }

      if (!pclce || !pclce.available || !pclce.identify) {
        setPclceIdentifierStatus("未找到 PCLCE");
      } else {
        const current = els.pclceIdentifier.value.trim();
        const previousAuto = localStorage.getItem(PCLCE_AUTO_KEY);
        const canUsePclceValue = !current || current.startsWith("WEB-") || current === previousAuto;
        if (canUsePclceValue) {
          els.pclceIdentifier.value = pclce.identify;
          localStorage.setItem(PCLCE_STORAGE_KEY, pclce.identify);
          localStorage.setItem(PCLCE_AUTO_KEY, pclce.identify);
          setPclceIdentifierStatus("已读取 PCLCE", "is-connected");
        } else {
          setPclceIdentifierStatus("PCLCE 可用 · 保留手动值", "is-connected");
        }
      }
      calculate();
    } catch {
      setIdentifierStatus("网页模式");
      setPclceIdentifierStatus("网页模式");
    }
  }

  function levelForScore(score) {
    if (score === 100) return { title: "满分运势", label: "100 人品", className: "status-perfect", message: "今天的好运已经拉满，适合做重要决定。" };
    if (score >= 90) return { title: "大吉", label: "大吉", className: "status-high", message: "状态很顺，值得把握今天的好机会。" };
    if (score >= 65) return { title: "顺风局", label: "顺风", className: "status-high", message: "整体手感不错，推进计划会更顺利。" };
    if (score >= 50) return { title: "平稳", label: "平稳", className: "status-even", message: "发挥稳定，按自己的节奏来就好。" };
    if (score >= 30) return { title: "小心行事", label: "谨慎", className: "status-low", message: "先稳住节奏，重要事项多检查一次。" };
    return { title: "低潮", label: "低潮", className: "status-low", message: "适合整理和休息，把高风险决定往后放。" };
  }

  function rowClassForScore(score) {
    if (score === 100) return "score-perfect";
    if (score >= 65) return "score-high";
    if (score >= 50) return "score-even";
    return "score-low";
  }

  function findPerfectDates(startDate, identifier, scorer, limit = 12) {
    const dates = [];
    let cursor = new Date(startDate);
    for (let offset = 0; offset < 3660 && dates.length < limit; offset += 1) {
      if (scorer(cursor, identifier) === 100) dates.push(new Date(cursor));
      cursor = addDays(cursor, 1);
    }
    return dates;
  }

  function renderScore(date, pcl2Identifier, pclceIdentifier) {
    const score = scoreForDate(date, pcl2Identifier);
    const pclceScore = pclceScoreForDate(date, pclceIdentifier);
    const level = levelForScore(score);
    const pclceLevel = levelForScore(pclceScore);
    els.heroDate.textContent = formatDate(date, true);
    els.scoreDateLabel.textContent = dateKey(date) === dateKey(new Date()) ? "今日人品" : "选定日期人品";
    els.scoreStatus.textContent = level.label;
    els.scoreStatus.className = `score-status ${level.className}`;
    els.scoreRing.style.setProperty("--score-angle", `${score * 3.6}deg`);
    els.todayScore.textContent = score;
    els.scoreMessageTitle.textContent = level.title;
    els.scoreMessage.textContent = level.message;
    els.scoreScaleFill.style.width = `${score}%`;
    els.pclceTodayScore.textContent = pclceScore;
    els.pclceScoreStatus.textContent = pclceLevel.label;
    els.pclceScoreStatus.className = `score-status ${pclceLevel.className}`;
    return { pcl2Score: score, pclceScore };
  }

  function renderTable(rows, startDate) {
    els.forecastBody.innerHTML = "";
    els.emptyState.hidden = rows.length > 0;
    rows.forEach(({ date, pcl2Score, pclceScore }) => {
      const level = levelForScore(pcl2Score);
      const row = document.createElement("tr");
      if (dateKey(date) === dateKey(new Date())) row.classList.add("is-today");
      if (pcl2Score === 100 || pclceScore === 100) row.classList.add("is-perfect");
      row.innerHTML = `
        <td>${formatDate(date)}${dateKey(date) === dateKey(new Date()) ? '<span class="date-sub">今天</span>' : ""}</td>
        <td>${WEEKDAYS[date.getDay()]}</td>
        <td class="score-value ${rowClassForScore(pcl2Score)}">${pcl2Score}</td>
        <td class="score-value ${rowClassForScore(pclceScore)}">${pclceScore}</td>
        <td class="trend-col"><div class="trend-track"><div class="trend-fill ${rowClassForScore(pcl2Score)}" style="width:${pcl2Score}%"></div></div></td>
        <td class="trend-col"><div class="trend-track"><div class="trend-fill ${rowClassForScore(pclceScore)}" style="width:${pclceScore}%"></div></div></td>
        <td><span class="status-label ${level.className}">${level.label}</span></td>`;
      row.addEventListener("click", () => {
        els.startDate.value = toDateInputValue(date);
        calculate();
        window.scrollTo({ top: 0, behavior: "smooth" });
      });
      els.forecastBody.appendChild(row);
    });
  }

  function renderPerfectList(element, dates) {
    element.innerHTML = "";
    if (dates.length === 0) {
      element.innerHTML = '<span class="perfect-empty">未来十年内没有查到满分日。</span>';
      return;
    }
    dates.slice(0, 8).forEach((date) => {
      const item = document.createElement("div");
      item.className = "perfect-date";
      item.innerHTML = `<strong>${formatDate(date)}</strong><span>${WEEKDAYS[date.getDay()]} · 100 人品</span>`;
      element.appendChild(item);
    });
  }

  function renderPerfectDates(pcl2Dates, pclceDates, startDate, rows) {
    els.pcl2PerfectCount.textContent = rows.filter((row) => row.pcl2Score === 100).length;
    els.pclcePerfectCount.textContent = rows.filter((row) => row.pclceScore === 100).length;
    els.pcl2WindowRange.textContent = `未来 ${rows.length} 天 · 起始 ${formatDate(startDate)}`;
    els.pclceWindowRange.textContent = `未来 ${rows.length} 天 · 起始 ${formatDate(startDate)}`;
    renderPerfectList(els.pcl2PerfectList, pcl2Dates);
    renderPerfectList(els.pclcePerfectList, pclceDates);
  }

  function calculate() {
    const date = dateFromInput(els.startDate.value);
    const pcl2Identifier = els.identifier.value.trim() || "WEB";
    const pclceIdentifier = els.pclceIdentifier.value.trim() || "WEB";
    localStorage.setItem(STORAGE_KEY, pcl2Identifier);
    localStorage.setItem(PCLCE_STORAGE_KEY, pclceIdentifier);
    const days = Number(els.daysCount.value);
    const rows = Array.from({ length: days }, (_, index) => {
      const rowDate = addDays(date, index);
      return {
        date: rowDate,
        pcl2Score: scoreForDate(rowDate, pcl2Identifier),
        pclceScore: pclceScoreForDate(rowDate, pclceIdentifier)
      };
    });
    renderScore(date, pcl2Identifier, pclceIdentifier);
    renderTable(rows, date);

    const nextPcl2Perfect = findPerfectDates(date, pcl2Identifier, scoreForDate, 1)[0];
    const nextPclcePerfect = findPerfectDates(date, pclceIdentifier, pclceScoreForDate, 1)[0];
    const firstPcl2Row = rows.reduce((best, row) => row.pcl2Score > best.pcl2Score ? row : best, rows[0]);
    const firstPclceRow = rows.reduce((best, row) => row.pclceScore > best.pclceScore ? row : best, rows[0]);
    const nextDiff = (perfectDate) => perfectDate ? Math.round((perfectDate - date) / 86400000) : 0;
    const nextLabel = (perfectDate) => perfectDate ? formatDate(perfectDate) : "未找到";
    const nextCountdown = (perfectDate) => perfectDate ? (nextDiff(perfectDate) === 0 ? "就是今天" : `${nextDiff(perfectDate)} 天后 · 100 人品`) : "未来十年内未出现";
    els.pcl2NextPerfectDate.textContent = nextLabel(nextPcl2Perfect);
    els.pcl2NextPerfectCountdown.textContent = nextCountdown(nextPcl2Perfect);
    els.pclceNextPerfectDate.textContent = nextLabel(nextPclcePerfect);
    els.pclceNextPerfectCountdown.textContent = nextCountdown(nextPclcePerfect);
    els.pcl2WindowHighScore.textContent = firstPcl2Row.pcl2Score;
    els.pcl2WindowHighDate.textContent = `${formatDate(firstPcl2Row.date)} · ${levelForScore(firstPcl2Row.pcl2Score).label}`;
    els.pclceWindowHighScore.textContent = firstPclceRow.pclceScore;
    els.pclceWindowHighDate.textContent = `${formatDate(firstPclceRow.date)} · ${levelForScore(firstPclceRow.pclceScore).label}`;
    renderPerfectDates(
      findPerfectDates(date, pcl2Identifier, scoreForDate, 12),
      findPerfectDates(date, pclceIdentifier, pclceScoreForDate, 12),
      date,
      rows
    );
  }

  function reset() {
    els.startDate.value = toDateInputValue(new Date());
    els.daysCount.value = "30";
    els.identifier.value = localStorage.getItem(PCL2_AUTO_KEY) || getDefaultIdentifier(STORAGE_KEY);
    els.pclceIdentifier.value = localStorage.getItem(PCLCE_AUTO_KEY) || getDefaultIdentifier(PCLCE_STORAGE_KEY);
    calculate();
  }

  els.startDate.value = toDateInputValue(new Date());
  initTheme();
  els.identifier.value = getDefaultIdentifier(STORAGE_KEY);
  els.pclceIdentifier.value = getDefaultIdentifier(PCLCE_STORAGE_KEY);
  themeToggle.addEventListener("click", () => {
    applyTheme(!document.documentElement.classList.contains("dark"));
  });
  $("#calculate-button").addEventListener("click", calculate);
  $("#reset-button").addEventListener("click", reset);
  els.startDate.addEventListener("change", calculate);
  els.daysCount.addEventListener("change", calculate);
  els.identifier.addEventListener("input", () => {
    setIdentifierStatus("手动标识", "is-manual");
    calculate();
  });
  els.pclceIdentifier.addEventListener("input", () => {
    setPclceIdentifierStatus("手动标识", "is-manual");
    calculate();
  });
  calculate();
  loadIdentifiers();
})();
