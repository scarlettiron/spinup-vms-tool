<!--
  Vmynt
  Licensed under the MIT License. See the LICENSE file in the project root.
  Copyright (c) 2026 Scarlett A. Scott (codescarlett)

  Contributors:
  Scarlett A. Scott (codescarlett)
-->

# Site theme

Everything that gives the Vmynt site its look lives in this folder, so it can be copied into
another repository or site as is. The pages only link to these files. They hold no styles or
scripts of their own, and nothing here loads from a CDN or a font service.

| File       | What it is                                                                                                              |
| ---------- | ----------------------------------------------------------------------------------------------------------------------- |
| `site.css` | Shared by every page: color tokens, base type, skip link, top bar, footer, pills, badges, buttons, code blocks, tables, callouts, tabs, cards, steps |
| `docs.css` | Page layouts: the single column `.page` with the home page hero, and the `.layout` with a sidebar used by the docs       |
| `demo.css` | The animated terminal                                                                                                   |
| `site.js`  | Copy buttons, the active sidebar link, tabs (with operating system detection), and playing the terminals               |
| `logo/`    | The logo set (see below)                                                                                                |

## Using it in another repository

1. Copy this whole folder next to your page, for example to `theme/`.
2. Link `site.css` first, then `docs.css`, then `demo.css` if the page has a terminal.
3. Paste the top bar and footer markup below, and change the links.
4. Load `site.js` at the end of `<body>`.

To change the colors, edit the custom properties at the top of `site.css`. Every rule reads from
them. `--primary` is the mint accent and `--on-primary` is the text color used on top of it (dark,
because white text on mint is hard to read).

```html
<head>
  <link rel="icon" href="./theme/logo/favicon.svg" type="image/svg+xml" />
  <link rel="icon" type="image/png" sizes="32x32" href="./theme/logo/favicon-32.png" />
  <link rel="apple-touch-icon" href="./theme/logo/apple-touch-icon.png" />
  <link rel="stylesheet" href="./theme/site.css" />
  <link rel="stylesheet" href="./theme/docs.css" />
  <link rel="stylesheet" href="./theme/demo.css" />
</head>
<body>
  <!--Hidden until a keyboard user tabs to it.-->
  <a class="skip-link" href="#content">Skip to content</a>
  <header class="site">
    <a class="brand" href="./" aria-label="Vmynt home">
      <img src="./theme/logo/wordmark-header.png" alt="Vmynt" width="288" height="96" />
    </a>
    <nav class="links" aria-label="Main">
      <a href="./docs/" aria-current="page">Docs</a>
      <!--hide-sm links are hidden on phones to keep the bar on one line.-->
      <a class="hide-sm" href="./commands/">Commands</a>
      <a href="https://github.com/scarlettiron/vmynt">GitHub</a>
    </nav>
  </header>

  <!--page content, with id="content" on its outer element-->

  <footer>
    <div class="sitelinks">
      <div>
        <h2>Learn</h2>
        <ul>
          <li><a href="./docs/">Getting started</a></li>
        </ul>
      </div>
    </div>
    <p>Footer text.</p>
    <!--Author signature, always the last thing on the page.-->
    <a class="made-by" href="https://codescarlett.com" rel="author">
      <img src="./theme/logo/codescarlett-dragonfly.svg" alt="" width="46" height="31" />
      <span><strong>Scarlett A. Scott</strong><span class="handle">codescarlett</span></span>
    </a>
  </footer>
  <script src="./theme/site.js"></script>
</body>
```

## Layouts

```html
<!--One centered column. The home page puts a .hero section first.-->
<div class="page" id="content">
  <main>
    <section class="hero">
      <div>
        <p class="eyebrow">Small label</p>
        <h1>Big title with an <em>accented</em> word</h1>
        <p class="lede">Intro paragraph.</p>
      </div>
      <div><!--a terminal, or an image--></div>
    </section>
  </main>
</div>

<!--A sidebar and the content. Sidebar links to "#id" light up as you scroll.
    On phones the sidebar turns into a row of links you can swipe sideways.-->
<div class="layout" id="content">
  <nav class="side" aria-label="On this page">
    <div class="group">Group label</div>
    <a href="#first">First</a>
  </nav>
  <main>
    <section id="first">
      <p class="crumbs"><a href="../">Vmynt</a> / Docs</p>
      <h1>Title</h1>
    </section>
  </main>
</div>
```

## Components

```html
<p class="callout"><strong>Heads up.</strong> A highlighted note.</p>
<p class="callout warn"><strong>Careful.</strong> A yellow note, for "not built yet" and cautions.</p>

<div class="pill-row"><span class="pill">A short label</span></div>

<span class="badge ok">available</span>
<span class="badge planned">coming in 1.0</span>

<div class="cta-row">
  <a class="cta" href="./install/">Main action</a>
  <a class="cta ghost" href="./docs/">Second action</a>
</div>

<div class="code">
  <div class="bar"><span>terminal</span><button class="copy" type="button">Copy</button></div>
  <pre>vmynt init</pre>
</div>

<!--Wrap wide tables so they scroll sideways on phones.-->
<div class="table-wrap"><table>...</table></div>

<!--A grid of cards. A card can also be a link: <a class="card" href="...">.-->
<div class="cards">
  <div class="card"><h3>Title</h3><p>Text.</p></div>
</div>

<!--Numbered steps with round counters.-->
<ol class="steps">
  <li><strong>Step title</strong> What happens in this step.</li>
</ol>

<!--A command heading with a badge, and a list of flags.-->
<div class="cmd-head"><h2>vmynt stop</h2> <span class="badge planned">coming in 1.0</span></div>
<p class="aliases">Aliases: <code>halt</code>, <code>down</code></p>
<dl class="flags">
  <dt>--force</dt>
  <dd>What the flag does.</dd>
</dl>
```

Inside a code block, wrap tokens in `tok-c` (comment), `tok-k` (keyword or command), `tok-s`
(string), or `tok-f` (function, type or flag) spans to color them.

### Tabs

Give each tab `data-os="windows"`, `"macos"` or `"linux"` and `site.js` opens the one that matches
the visitor's computer. Arrow keys, Home and End move between tabs. Each tab and panel needs a
unique `id`.

```html
<div class="tabs">
  <div role="tablist" aria-label="Your operating system">
    <button role="tab" id="tab-win" aria-controls="panel-win" aria-selected="true" data-os="windows">Windows</button>
    <button role="tab" id="tab-mac" aria-controls="panel-mac" aria-selected="false" data-os="macos" tabindex="-1">macOS</button>
  </div>
  <div role="tabpanel" id="panel-win" aria-labelledby="tab-win">...</div>
  <div role="tabpanel" id="panel-mac" aria-labelledby="tab-mac" hidden>...</div>
</div>
```

### Animated terminal

Write the terminal as it looks when finished. `site.js` types each `.cmd` out letter by letter and
shows everything else all at once, starting when the terminal scrolls into view. With reduced motion
turned on, or without JavaScript, the finished terminal simply shows as written, so it has to read
well on its own. Keep each output block in one `<span>` so it appears together.

```html
<div class="term">
  <div class="term-bar">
    <span class="dot"></span><span class="dot"></span><span class="dot"></span>
    <span class="term-title">~/work/acme</span>
    <button class="replay" type="button">Replay</button>
  </div>
  <pre aria-label="What the terminal shows, in one sentence"><span class="prompt">$ </span><span class="cmd">vmynt --version</span>
<span class="out">vmynt 0.1.0</span></pre>
</div>
```

Output colors: `ok` (mint), `dim`, `warn`, `err`, `hl` (blue).

## Embedding in another site

The pages are plain static files with relative links, so the whole site folder can be served from
any path or domain. To put it on a subdomain of another site (the way codescarlett.com serves its
project sites):

1. Copy the site folder into the other site's static files, for example `public/vmynt/`.
2. Rewrite the absolute Pages URL, `https://scarlettiron.github.io/vmynt/`, to the new address in
   the `.html`, `.xml` and `.txt` files. It appears in canonical links, Open Graph and JSON-LD,
   `robots.txt`, `sitemap.xml` and `404.html`. Relative links need no change.
3. Route the subdomain to that folder.

Copy only this `theme/` folder to reuse the look on unrelated pages.

## Logo

| File                         | Use it for                                                            |
| ---------------------------- | --------------------------------------------------------------------- |
| `vmynt-icon.svg`             | The icon (a VM window with a sprout), any size                        |
| `vmynt-wordmark.svg`         | Icon plus "vmynt", for dark backgrounds. Uses system fonts for the text |
| `vmynt-icon.png`             | The icon at 512px                                                     |
| `wordmark-header.png`        | The wordmark at 3x the size the top bar shows it                      |
| `favicon.svg`                | Browser tab icon for modern browsers                                  |
| `favicon-32.png`             | Browser tab icon, fallback                                            |
| `apple-touch-icon.png`       | iOS home screen icon, on a solid background because iOS fills transparency black |
| `og-image.png`               | 1200x630 social card for link previews                                |
| `codescarlett-dragonfly.svg` | The author's dragonfly mark, used in the footer signature             |

The SVGs are the masters. The PNGs are drawn from the same 64-unit geometry; make any new size from
the SVG rather than from one of the small PNGs.
