/**
 * BeThere's look for the views (plan 024), loaded into the CSS override
 * editor in one click. Colours and fonts are BeThere's attendee site
 * (`frontend-leptos/styles/style-23-landing.css`): navy for a screen in a
 * dark room, with its paper version below for a bright one.
 */
export const bethereThemeCss = `/**
 * BeThere theme (plan 024): the colours and fonts of BeThere's event pages,
 * so the screens in the room and the stream match where people registered.
 * Navy is set below. For a bright room, swap in the paper values at the end.
 */
@import url('https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700;800&family=Anuphan:wght@400;600;700&display=swap');

:root {
  --background-color-override: #12152a;
  --card-background-color-override: #171b33;
  --color-override: #e9e4d3;
  --secondary-color-override: #b9b6ad;
  --label-color-override: #8f88ff;
  --accent-color-override: #8f88ff;

  --timer-color-override: #e9e4d3;
  --timer-warning-color-override: #f5b544;
  --timer-danger-color-override: #f0666b;
  --timer-overtime-color-override: #f0666b;
  --timer-pending-color-override: #8f88ff;

  /** Thai text falls back to Anuphan, as on BeThere */
  --font-family-override: 'Inter', 'Anuphan', system-ui, sans-serif;
  --external-color-override: #e9e4d3;

  /** /backstage */
  --card-background-color-blink-override: #5a54cb;
  --timer-progress-bg-override: #2a3052;
  --timer-progress-override: #8f88ff;

  /** /op */
  --operator-running-bg-override: #5a54cb;

  /** /studio */
  --studio-active: #e9e4d3;
  --studio-idle: #2a3052;
  --studio-active-label: #e9e4d3;
  --studio-idle-label: #b9b6ad;
  --studio-overtime: #f0666b;
}

/** /timeline: what is on now in BeThere indigo, the time to each next one in its green */
.timeline .section-content--now .title-card__title {
  color: #8f88ff;
}
.timeline .section-title__status {
  color: #4fd68a;
}

/**
 * Paper, for a bright room: replace the matching lines above with these.
 *   --background-color-override: #f4f0e6;
 *   --card-background-color-override: #ebe5d6;
 *   --color-override: #121212;
 *   --secondary-color-override: #4a4740;
 *   --label-color-override: #5a54cb;
 *   --accent-color-override: #5a54cb;
 *   --timer-color-override: #121212;
 *   --timer-warning-color-override: #8a5300;
 *   --timer-danger-color-override: #b42318;
 *   --timer-overtime-color-override: #b42318;
 *   --timer-pending-color-override: #5a54cb;
 *   --external-color-override: #121212;
 *   --timer-progress-bg-override: #ebe5d6;
 *   --timer-progress-override: #5a54cb;
 *   --studio-active: #121212;
 *   --studio-idle: #ebe5d6;
 *   --studio-active-label: #121212;
 *   --studio-idle-label: #4a4740;
 *   --studio-overtime: #b42318;
 * and in /timeline, #5a54cb for what is on now and #1f7a4d for the times.
 */
`;
