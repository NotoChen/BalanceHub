import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BrowserWorker, sameOriginUrl } from "./worker.mjs";

test("credentials are confined to the exact original origin", () => {
  assert.equal(sameOriginUrl("https://example.test", "/api/user/checkin"), "https://example.test/api/user/checkin");
  for (const target of [
    "https://other.test/api/user/checkin",
    "http://example.test/api/user/checkin",
    "https://example.test:444/api/user/checkin",
    "https://user:password@example.test/api/user/checkin",
    "//other.test/api/user/checkin",
  ]) {
    assert.throws(() => sameOriginUrl("https://example.test", target), /停止发送账号凭据/);
  }
});

test("real browser preserves authentication and HTTP status without following redirects", {
  skip: process.env.BALANCEHUB_BROWSER_SMOKE !== "1",
  timeout: 45_000,
}, async () => {
  const profile = await mkdtemp(join(tmpdir(), "balancehub-browser-smoke-"));
  let submissions = 0;
  let leakedRequests = 0;
  const external = createServer((_request, response) => {
    leakedRequests++;
    response.end("{}");
  });
  await new Promise((resolve) => external.listen(0, "127.0.0.1", resolve));
  const externalUrl = "http://127.0.0.1:" + external.address().port;
  const server = createServer((request, response) => {
    if (request.url === "/redirect") {
      response.writeHead(302, { location: externalUrl });
      response.end();
    } else if (request.url === "/limited") {
      response.writeHead(429, { "content-type": "application/json", "retry-after": "60" });
      response.end('{"success":false}');
    } else if (request.method === "POST") {
      submissions++;
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({
        authorization: request.headers.authorization,
        user: request.headers["new-api-user"],
      }));
    } else {
      response.setHeader("content-type", "application/json");
      response.end("{}");
    }
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const origin = "http://127.0.0.1:" + server.address().port;
  const worker = new BrowserWorker();
  try {
    await worker.open({ url: origin + "/api/status", profileDir: profile, proxy: { direct: true }, executablePath: process.env.BALANCEHUB_BROWSER_EXECUTABLE });
    const headers = { authorization: "Bearer fixture-token", "new-api-user": "fixture-user" };
    const result = await worker.fetch({ path: "/api/user/checkin", method: "POST", headers });
    assert.equal(result.status, 200);
    assert.deepEqual(JSON.parse(result.body), { authorization: headers.authorization, user: headers["new-api-user"] });
    const limited = await worker.fetch({ path: "/limited", headers });
    assert.equal(limited.status, 429);
    assert.equal(limited.headers["retry-after"], "60");
    await assert.rejects(worker.fetch({ path: "/redirect", headers }));
    assert.equal(leakedRequests, 0);
    assert.equal(submissions, 1);
  } finally {
    await worker.close();
    await Promise.all([
      new Promise((resolve) => server.close(resolve)),
      new Promise((resolve) => external.close(resolve)),
    ]);
    await rm(profile, { recursive: true, force: true });
  }
});
