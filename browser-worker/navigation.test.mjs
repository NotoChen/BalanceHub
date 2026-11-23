import assert from "node:assert/strict";
import test from "node:test";
import vm from "node:vm";
import { keepOAuthInWindow } from "./launch.mjs";
import { approveConsentButton } from "./login.mjs";

function navigationSandbox(href = "https://relay.test/login") {
  const listeners = {};
  const assigned = [];
  const nativeOpened = [];
  const sandbox = {
    URL,
    String,
    location: {
      href,
      origin: new URL(href).origin,
      assign: (url) => assigned.push(url),
    },
    document: {
      addEventListener: (type, handler) => { listeners[type] = handler; },
    },
  };
  sandbox.window = sandbox;
  sandbox.top = sandbox;
  sandbox.open = (...args) => { nativeOpened.push(args); return { native: true }; };
  vm.createContext(sandbox);
  vm.runInContext(`(${keepOAuthInWindow.toString()})()`, sandbox);
  return { sandbox, listeners, assigned, nativeOpened };
}

test("app-window OAuth popups and blank-target links stay in the controlled window", () => {
  const { sandbox, listeners, assigned, nativeOpened } = navigationSandbox();
  const authorize = "https://connect.linux.do/oauth2/authorize?client_id=fixture";
  assert.ok(sandbox.open(authorize));
  assert.deepEqual(assigned, [authorize]);
  assert.equal(nativeOpened.length, 0, "the OAuth URL must not escape to the system browser");

  const github = "https://github.com/login/oauth/authorize?client_id=fixture";
  sandbox.open(github);
  assert.deepEqual(assigned, [authorize, github]);

  sandbox.open("https://relay.test/api/oauth/linuxdo");
  assert.equal(assigned.length, 3, "same-origin OAuth endpoints also stay in the window");

  sandbox.open("https://relay.test/docs");
  assert.equal(nativeOpened.length, 1, "ordinary links keep the native behavior");

  const anchor = { target: "_blank", href: authorize, closest: () => anchor };
  const click = { target: anchor, prevented: false, preventDefault() { this.prevented = true; } };
  listeners.click(click);
  assert.equal(click.prevented, true);
  assert.equal(assigned.at(-1), authorize);

  const form = { target: "_blank", action: authorize };
  listeners.submit({ target: form });
  assert.equal(form.target, "_self");
});

function consentSandbox(hostname, pathname, buttons) {
  const sandbox = {
    location: { hostname, pathname },
    document: { querySelectorAll: () => buttons },
    getComputedStyle: () => ({ visibility: "visible", display: "block" }),
  };
  vm.createContext(sandbox);
  return sandbox;
}

function consentButton(text, { disabled = false, width = 80, height = 30 } = {}) {
  return {
    textContent: text,
    value: "",
    disabled,
    clicked: false,
    getBoundingClientRect: () => ({ width, height }),
    click() { this.clicked = true; },
  };
}

test("only the official authorization button is clicked automatically", () => {
  const allow = consentButton("允许");
  assert.equal(
    vm.runInContext(`(${approveConsentButton.toString()})()`, consentSandbox("connect.linux.do", "/oauth2/approve/fixture", [consentButton("拒绝"), allow])),
    true,
  );
  assert.equal(allow.clicked, true);

  // 站点把按钮写成「允许访问」时同样是正式授权入口。
  const allowAccess = consentButton("允许访问");
  assert.equal(
    vm.runInContext(`(${approveConsentButton.toString()})()`, consentSandbox("linux.do", "/oauth2/approve/fixture", [allowAccess])),
    true,
  );
  assert.equal(allowAccess.clicked, true);

  const hidden = consentButton("允许", { width: 0, height: 0 });
  assert.equal(
    vm.runInContext(`(${approveConsentButton.toString()})()`, consentSandbox("connect.linux.do", "/oauth2/approve/fixture", [hidden])),
    false,
  );

  const github = consentButton("Authorize Agent Router");
  assert.equal(
    vm.runInContext(`(${approveConsentButton.toString()})()`, consentSandbox("github.com", "/login/oauth/authorize", [github])),
    true,
  );
  assert.equal(github.clicked, true);

  const otherPath = consentButton("Authorize Agent Router");
  assert.equal(
    vm.runInContext(`(${approveConsentButton.toString()})()`, consentSandbox("github.com", "/settings/applications", [otherPath])),
    false,
  );
  assert.equal(
    vm.runInContext(`(${approveConsentButton.toString()})()`, consentSandbox("example.test", "/oauth2/approve/fixture", [consentButton("允许")])),
    false,
  );
});
