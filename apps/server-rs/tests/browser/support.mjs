// Shared by the browser checks: a browser whose every page logs the requests
// the server refused, so a console "Failed to load resource" names its URL.
import { chromium } from '@playwright/test';

const logRefused = (target) =>
  target.on('response', (response) => {
    if (response.status() < 400) return;
    const url = new URL(response.url());
    console.log(`    ${response.status()} ${response.request().method()} ${url.pathname}${url.search}`);
  });

export async function launch() {
  const browser = await chromium.launch();
  const newContext = browser.newContext.bind(browser);
  const newPage = browser.newPage.bind(browser);
  browser.newContext = async (...options) => {
    const context = await newContext(...options);
    logRefused(context);
    return context;
  };
  browser.newPage = async (...options) => {
    const page = await newPage(...options);
    logRefused(page);
    return page;
  };
  return browser;
}
