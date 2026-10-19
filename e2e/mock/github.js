// A mock GitHub for the WebDriver suite: answers the app's GraphQL queries (told apart by
// their shape, as `projects/remote.rs`, `status.rs` and `groups.rs` build them), the REST profile,
// avatars, the Pages manifests and the `result.json` assets, from a `world.js` fixture.
// The app reaches it through the `C50_TEST_GITHUB` hook (`src-tauri/src/testenv.rs`). Node's
// standard library only, so `pnpm test` can check it without the suite's dependencies.

import { createServer } from "node:http";

/** A 1x1 transparent PNG, for avatars. */
const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=",
  "base64",
);

/** Which of the app's queries `query` is; `null` for one the mock doesn't know. */
export function queryKind(query) {
  if (/\brelease\(tagName:/.test(query)) return "releaseBody";
  if (/\borganizations\(/.test(query)) return "teams";
  if (/\brepositories\(/.test(query)) return "list";
  if (/\bg\d+: organization\(/.test(query)) return "groups";
  if (/\br\d+: repository\(/.test(query)) {
    return /\.\.\.Status\b/.test(query) ? "status" : "details";
  }
  return null;
}

const webUrl = (repo) => `https://github.com/${repo.owner}/${repo.name}`;
const findRepo = (world, owner, name) =>
  world.repos.find(
    (r) =>
      r.owner.toLowerCase() === owner?.toLowerCase() &&
      r.name.toLowerCase() === name?.toLowerCase(),
  );

/** The `Repo` fragment of `projects/remote.rs`. The student made every commit. */
function repoNode(repo, login) {
  return {
    name: repo.name,
    owner: { login: repo.owner },
    url: webUrl(repo),
    isArchived: false,
    viewerPermission: repo.permission ?? "ADMIN",
    defaultBranchRef: {
      name: "main",
      target: {
        oid: repo.head.oid,
        committedDate: repo.head.committedDate,
        author: { name: "Ada E2E", user: { login } },
      },
    },
    config: repo.config === null ? null : { text: repo.config },
    submissions: {
      nodes: [...repo.submissions]
        .sort((a, b) => b.tag.localeCompare(a.tag))
        .slice(0, 1)
        .map((s) => ({
          name: s.tag.replace(/^submit\//, ""),
          // An annotated tag: the tag object, then the commit it points at.
          target: { oid: "a".repeat(40), target: { oid: s.commit } },
        })),
    },
  };
}

/** The `Status` fragment of `status.rs`, on top of `Repo`. */
function statusNode(repo, login, base) {
  return {
    ...repoNode(repo, login),
    branches: { nodes: [{ name: "main", compare: { behindBy: 0 } }] },
    releases: {
      nodes: [...repo.releases]
        .sort((a, b) => b.publishedAt.localeCompare(a.publishedAt))
        .slice(0, 10)
        .map((r) => ({
          tagName: r.tagName,
          isDraft: false,
          publishedAt: r.publishedAt,
          url: `${webUrl(repo)}/releases/tag/${r.tagName}`,
          result: {
            nodes: r.result ? [{ url: assetUrl(base, repo, r.tagName) }] : [],
          },
        })),
    },
    grading: { target: { autograde: null, checkSuites: { nodes: [] } } },
    feedback: { nodes: [] },
  };
}

function assetUrl(base, repo, tag) {
  return `${base}/assets/${repo.owner}/${repo.name}/${encodeURIComponent(tag)}/result.json`;
}

/** The `data` of a GraphQL answer. */
export function graphql(world, base, { query, variables = {} }) {
  switch (queryKind(query)) {
    case "list":
      return {
        viewer: {
          login: world.login,
          repositories: {
            pageInfo: { hasNextPage: false, endCursor: null },
            nodes: world.repos.map((r) => ({
              nameWithOwner: `${r.owner}/${r.name}`,
              pushedAt: r.pushedAt,
              isTemplate: false,
              isArchived: false,
              viewerPermission: r.permission ?? "ADMIN",
              templateRepository: r.template ? { nameWithOwner: r.template } : null,
            })),
          },
        },
      };
    case "teams":
      return {
        viewer: {
          organizations: {
            pageInfo: { hasNextPage: false, endCursor: null },
            nodes: world.orgs.map((org) => ({
              login: org.login,
              teams: { nodes: org.teams.map((t) => ({ ...t })) },
            })),
          },
        },
      };
    case "details":
    case "status": {
      const data = {};
      for (const alias of query.match(/\br\d+(?=: repository\()/g) ?? []) {
        const i = alias.slice(1);
        const repo = findRepo(world, variables[`o${i}`], variables[`n${i}`]);
        data[alias] = !repo
          ? null
          : queryKind(query) === "status"
            ? statusNode(repo, world.login, base)
            : repoNode(repo, world.login);
      }
      return data;
    }
    case "groups": {
      // The world has no group assignments.
      const data = {};
      for (const alias of query.match(/\bg\d+(?=: organization\()/g) ?? []) data[alias] = null;
      return data;
    }
    case "releaseBody": {
      const repo = findRepo(world, variables.owner, variables.name);
      const release = repo?.releases.find((r) => r.tagName === variables.tag);
      return {
        repository: repo ? { release: release ? { description: release.body } : null } : null,
      };
    }
    default:
      return null;
  }
}

const RATE_LIMIT = {
  "x-ratelimit-limit": "5000",
  "x-ratelimit-remaining": "4990",
  "x-ratelimit-reset": String(Math.floor(Date.now() / 1000) + 3600),
};

/**
 * Answers one request: `{ status, headers, body }`, or `null` for 404.
 * @param {object} world `buildWorld()`
 * @param {string} base the mock's own URL, for the asset links it hands out
 * @param {{ method: string, url: string, headers?: Record<string, string>, body?: string }} req
 */
export function respond(world, base, req) {
  const url = new URL(req.url, base);
  const path = url.pathname;
  const json = (value, extra = {}) => ({
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8", ...extra },
    body: JSON.stringify(value),
  });

  if (req.method === "POST" && path === "/graphql") {
    if (!req.headers?.authorization?.startsWith("Bearer ")) {
      return { status: 401, headers: {}, body: '{"message":"Bad credentials"}' };
    }
    let request;
    try {
      request = JSON.parse(req.body ?? "");
    } catch {
      return json({ errors: [{ message: "mock: not JSON" }] });
    }
    const data = graphql(world, base, request);
    if (data === null) {
      return json({ data: null, errors: [{ message: "mock: unknown query" }] }, RATE_LIMIT);
    }
    return json({ data }, RATE_LIMIT);
  }
  if (req.method !== "GET") return null;

  let match = path.match(/^\/users\/([A-Za-z0-9-]+)$/);
  if (match) return json({ login: match[1], email: null }, RATE_LIMIT);

  if (/^\/web\/[A-Za-z0-9-]+\.png$/.test(path)) {
    return { status: 200, headers: { "content-type": "image/png" }, body: PNG };
  }

  match = path.match(/^\/pages\/([^/]+)\/classroom50\/([^/]+)\/assignments\.json$/);
  if (match) {
    const [, org, classroom] = match;
    const key = Object.keys(world.manifests).find(
      (k) => k.toLowerCase() === `${org}/${classroom}`.toLowerCase(),
    );
    if (!key) return null;
    const body = JSON.stringify(world.manifests[key]);
    const etag = `"${Buffer.from(body).length}-${key}"`;
    if (req.headers?.["if-none-match"] === etag) {
      return { status: 304, headers: { etag }, body: "" };
    }
    return { status: 200, headers: { "content-type": "application/json", etag }, body };
  }

  match = path.match(/^\/assets\/([^/]+)\/([^/]+)\/([^/]+)\/result\.json$/);
  if (match) {
    const repo = findRepo(world, match[1], match[2]);
    const tag = decodeURIComponent(match[3]);
    const release = repo?.releases.find((r) => r.tagName === tag);
    return release?.result ? json(release.result) : null;
  }
  return null;
}

/**
 * Starts the mock on 127.0.0.1. `requests` lists what was asked (method, path, GraphQL kind),
 * also served at `GET /__requests` for the specs, which run in another process.
 * @param {object} world `buildWorld()`
 * @param {{ port?: number }} [options]
 */
export function start(world, { port = 0 } = {}) {
  /** @type {{ method: string, path: string, kind: string | null }[]} */
  const requests = [];
  let base = "";
  const server = createServer((req, res) => {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      const body = Buffer.concat(chunks).toString("utf8");
      const path = (req.url ?? "/").split("?")[0];
      if (path === "/__requests") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify(requests));
        return;
      }
      let kind = null;
      if (path === "/graphql") {
        try {
          kind = queryKind(JSON.parse(body).query ?? "");
        } catch {
          kind = null;
        }
      }
      requests.push({ method: req.method ?? "GET", path, kind });
      const headers = /** @type {Record<string, string>} */ (req.headers);
      const answer = respond(world, base, {
        method: req.method ?? "GET",
        url: req.url ?? "/",
        headers,
        body,
      });
      if (!answer) {
        res.writeHead(404, { "content-type": "application/json" });
        res.end('{"message":"Not Found"}');
        return;
      }
      res.writeHead(answer.status, answer.headers);
      res.end(answer.body);
    });
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      const address = server.address();
      base = `http://127.0.0.1:${typeof address === "object" && address ? address.port : port}`;
      resolve({
        url: base,
        requests,
        close: () => new Promise((done) => server.close(() => done(undefined))),
      });
    });
  });
}
