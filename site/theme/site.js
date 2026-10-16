//Vmseed
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Small behaviors shared by every page. Loaded at the end of <body>, so the
//elements it looks for already exist. Every part is optional: a page without
//tabs or terminals simply has nothing for that part to do.
(function () {
  'use strict';

  //True when the visitor asked their system for less animation.
  const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  //----------------------------------------------------------------------------
  //Copy buttons on code blocks.
  //----------------------------------------------------------------------------
  document.querySelectorAll('.code .copy').forEach((button) => {
    button.addEventListener('click', () => {
      const pre = button.closest('.code').querySelector('pre');
      navigator.clipboard.writeText(pre.innerText.trim()).then(() => {
        const original = button.textContent;
        button.textContent = 'Copied';
        setTimeout(() => {
          button.textContent = original;
        }, 1500);
      });
    });
  });

  //----------------------------------------------------------------------------
  //Highlight the sidebar link for the section currently in view.
  //Only links that point at a section on this page ("#something") take part.
  //----------------------------------------------------------------------------
  const sideLinks = new Map();
  document.querySelectorAll('nav.side a[href^="#"]').forEach((link) => {
    sideLinks.set(link.getAttribute('href').slice(1), link);
  });

  if (sideLinks.size > 0 && 'IntersectionObserver' in window) {
    const sectionObserver = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (!entry.isIntersecting) return;
          const active = sideLinks.get(entry.target.id);
          if (!active) return;
          sideLinks.forEach((link) => link.classList.remove('active'));
          active.classList.add('active');
        });
      },
      { rootMargin: '-20% 0px -70% 0px' },
    );
    document.querySelectorAll('main section[id]').forEach((section) => sectionObserver.observe(section));
  }

  //----------------------------------------------------------------------------
  //Tabs. Markup: a .tabs box with one role="tablist" and one role="tabpanel"
  //per tab. A tab with data-os="windows|macos|linux" is chosen automatically
  //when it matches the visitor's computer.
  //----------------------------------------------------------------------------

  //Works out the visitor's operating system from the browser's user agent.
  function detectOs() {
    const agent = navigator.userAgent || '';
    if (/Windows/i.test(agent)) return 'windows';
    if (/Mac OS X|Macintosh/i.test(agent) && !/iPhone|iPad/i.test(agent)) return 'macos';
    if (/Linux|X11/i.test(agent) && !/Android/i.test(agent)) return 'linux';
    return null;
  }

  //Shows one tab's panel and hides the others in the same group.
  function selectTab(tab, moveFocus) {
    const list = tab.closest('[role="tablist"]');
    list.querySelectorAll('[role="tab"]').forEach((other) => {
      const selected = other === tab;
      other.setAttribute('aria-selected', String(selected));
      other.tabIndex = selected ? 0 : -1;
      const panel = document.getElementById(other.getAttribute('aria-controls'));
      if (panel) panel.hidden = !selected;
    });
    if (moveFocus) tab.focus();
  }

  const visitorOs = detectOs();
  document.querySelectorAll('.tabs [role="tablist"]').forEach((list) => {
    const tabs = Array.from(list.querySelectorAll('[role="tab"]'));

    tabs.forEach((tab, index) => {
      tab.addEventListener('click', () => selectTab(tab, false));

      //Arrow keys move between tabs, as screen reader users expect.
      tab.addEventListener('keydown', (event) => {
        let next = null;
        if (event.key === 'ArrowRight') next = tabs[(index + 1) % tabs.length];
        if (event.key === 'ArrowLeft') next = tabs[(index - 1 + tabs.length) % tabs.length];
        if (event.key === 'Home') next = tabs[0];
        if (event.key === 'End') next = tabs[tabs.length - 1];
        if (next) {
          event.preventDefault();
          selectTab(next, true);
        }
      });
    });

    const match = visitorOs && tabs.find((tab) => tab.dataset.os === visitorOs);
    const preselected = tabs.find((tab) => tab.getAttribute('aria-selected') === 'true');
    selectTab(match || preselected || tabs[0], false);
  });

  //----------------------------------------------------------------------------
  //Animated terminals. Inside a .term <pre>:
  //  <span class="cmd">...</span>  is typed out one character at a time
  //  anything else                 appears all at once, after a short pause
  //The finished terminal is what is in the HTML, so with reduced motion or no
  //JavaScript the visitor simply sees the end result.
  //----------------------------------------------------------------------------
  const TYPE_DELAY_MS = 38;
  const OUTPUT_DELAY_MS = 260;

  //Waits the given number of milliseconds.
  function wait(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }

  //Plays one terminal from empty to finished. Each run gets a number, so a
  //replay started half way through makes the older run stop quietly.
  async function playTerminal(term) {
    const pre = term.querySelector('pre');
    const run = (Number(term.dataset.run) || 0) + 1;
    term.dataset.run = String(run);
    const stillCurrent = () => term.dataset.run === String(run);

    //Put the finished HTML back first, then take it apart into steps.
    pre.innerHTML = term._finalHtml;
    const nodes = Array.from(pre.childNodes);
    const steps = nodes.map((node) => {
      const isCommand = node.nodeType === Node.ELEMENT_NODE && node.classList.contains('cmd');
      return { node, isCommand, text: node.textContent };
    });

    //Hide everything.
    pre.textContent = '';
    const cursor = document.createElement('span');
    cursor.className = 'cursor';
    cursor.setAttribute('aria-hidden', 'true');

    for (const step of steps) {
      if (!stillCurrent()) return;

      if (step.isCommand) {
        //Type the command out letter by letter, with the cursor after it.
        const typed = step.node.cloneNode(false);
        pre.appendChild(typed);
        pre.appendChild(cursor);
        for (const character of step.text) {
          if (!stillCurrent()) return;
          typed.textContent += character;
          await wait(TYPE_DELAY_MS);
        }
        cursor.remove();
        await wait(OUTPUT_DELAY_MS);
      } else {
        //Output and prompts appear whole. A pause only before real output,
        //not before every newline.
        const isOutput = step.node.nodeType === Node.ELEMENT_NODE && !step.node.classList.contains('prompt');
        if (isOutput) await wait(OUTPUT_DELAY_MS / 2);
        pre.appendChild(step.node);
      }
    }
  }

  document.querySelectorAll('.term').forEach((term) => {
    const pre = term.querySelector('pre');
    if (!pre) return;
    term._finalHtml = pre.innerHTML;

    const replay = term.querySelector('.replay');
    if (reduceMotion) {
      //Nothing to replay when nothing animates.
      if (replay) replay.hidden = true;
      return;
    }
    if (replay) replay.addEventListener('click', () => playTerminal(term));

    //Start each terminal the first time it scrolls into view.
    if ('IntersectionObserver' in window) {
      const startObserver = new IntersectionObserver(
        (entries) => {
          entries.forEach((entry) => {
            if (entry.isIntersecting) {
              startObserver.disconnect();
              playTerminal(term);
            }
          });
        },
        { threshold: 0.35 },
      );
      startObserver.observe(term);
    }
  });
})();
