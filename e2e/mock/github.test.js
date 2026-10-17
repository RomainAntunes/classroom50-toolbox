// The mock GitHub, checked without the app: it must recognize the queries the Rust code
// really sends (read from its source, so a changed query fails here first), answer them in
// the shapes Rust reads, and keep the golden answers in `golden/` current. Rust's own tests
// (`e2e_golden` in projects.rs, status.rs and classroom50.rs) deserialize those same files,
// so the two sides can't drift apart. `UPDATE_GOLDEN=1 pnpm test` rewrites them.

import { readFileSync, writeFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { graphql, queryKind, respond, start } from "./github.js";
import { buildWorld } from "./world.js";

const root = new URL("../../", import.meta.url);
const rust = (file) => readFileSync(new URL(`src-tauri/src/${file}`, root), "utf8");

/** A `const NAME: &str = r#"..."#;` from a Rust file. */
function rustConst(file, name) {
  const match = rust(file).match(new RegExp(`const ${name}: &str = r#"([\\s\\S]*?)"#;`));
  if (!match) throw new Error(`${name} not found in ${file}`);
  return match[1];
}

const LIST = rustConst("projects.rs", "LIST_QUERY");
const TEAMS = rustConst("projects.rs", "TEAMS_QUERY");
const REPO = rustConst("projects.rs", "REPO_FRAGMENT");
const STATUS = rustConst("status.rs", "FRAGMENT");
const BODY = rustConst("status.rs", "RELEASE_BODY");

/** `projects::fetch_nodes`'s aliased query. */
function nodesQuery(names, fragment, fragments) {
  const params = names.map((_, i) => `$o${i}: String!, $n${i}: String!`);
  const fields = names
    .map((_, i) => `r${i}: repository(owner: $o${i}, name: $n${i}) { ...${fragment} }\n`)
    .join("");
  const variables = {};
  names.forEach((full, i) => {
    const [owner, name] = full.split("/");
    variables[`o${i}`] = owner;
    variables[`n${i}`] = name;
  });
  return { query: `query(${params.join(", ")}) {\n${fields}}\n${fragments}`, variables };
}

/** `groups::members_query`'s aliased query, for one team. */
const GROUPS =
  "query($o0: String!, $s0: String!) { g0: organization(login: $o0) { team(slug: $s0) { members(first: 50) { edges { role node { login name } } } } } }";

const BASE = "http://127.0.0.1:4545";
const world = buildWorld({
  now: new Date("2026-10-16T12:00:00Z"),
  heads: { hello: "c".repeat(40), helloSubmitted: "b".repeat(40) },
});
const names = [...world.repos.map((r) => `${r.owner}/${r.name}`), "c50-e2e-school/gone"];
const helloTag = world.repos[0].releases[0].tagName;

describe("the mock GitHub", () => {
  it("knows every query the app sends", () => {
    expect(queryKind(LIST)).toBe("list");
    expect(queryKind(TEAMS)).toBe("teams");
    expect(queryKind(nodesQuery(names, "Repo", REPO).query)).toBe("details");
    expect(queryKind(nodesQuery(names, "Status", REPO + STATUS).query)).toBe("status");
    expect(queryKind(BODY)).toBe("releaseBody");
    expect(queryKind(GROUPS)).toBe("groups");
    expect(queryKind("query { viewer { email } }")).toBe(null);
    // The groups query the mock copies is the one Rust builds.
    expect(rust("groups.rs")).toContain("g{i}: organization(login: $o{i}) {{ team(slug: $s{i})");
  });

  it("answers like the golden files", () => {
    const answers = {
      list: graphql(world, BASE, { query: LIST, variables: { cursor: null } }),
      teams: graphql(world, BASE, { query: TEAMS, variables: { login: world.login } }),
      details: graphql(world, BASE, nodesQuery(names, "Repo", REPO)),
      status: graphql(world, BASE, nodesQuery(names, "Status", REPO + STATUS)),
      "release-body": graphql(world, BASE, {
        query: BODY,
        variables: { owner: "c50-e2e-school", name: "algo-hello-ada-e2e", tag: helloTag },
      }),
      "manifest-algo": world.manifests["c50-e2e-school/algo"],
      "manifest-web": world.manifests["c50-e2e-lab/web"],
      "result-hello": world.repos[0].releases[0].result,
    };
    for (const [name, answer] of Object.entries(answers)) {
      const file = new URL(`golden/${name}.json`, import.meta.url);
      if (process.env.UPDATE_GOLDEN) writeFileSync(file, `${JSON.stringify(answer, null, 2)}\n`);
      expect(answer, name).toEqual(JSON.parse(readFileSync(file, "utf8")));
    }
  });

  it("lists only the hello repo's grade and points at its asset", () => {
    const status = graphql(world, BASE, nodesQuery(names, "Status", REPO + STATUS));
    const url = status.r0.releases.nodes[0].result.nodes[0].url;
    expect(url).toBe(
      `${BASE}/assets/c50-e2e-school/algo-hello-ada-e2e/${encodeURIComponent(helloTag)}/result.json`,
    );
    const asset = respond(world, BASE, { method: "GET", url: new URL(url).pathname });
    expect(JSON.parse(String(asset?.body)).score).toBe(7);
    expect(status.r1.releases.nodes).toEqual([]);
    expect(status[`r${names.length - 1}`]).toBe(null);
  });

  it("serves manifests with an ETag and 304s a revalidation", () => {
    const url = "/pages/c50-e2e-school/classroom50/algo/assignments.json";
    const first = respond(world, BASE, { method: "GET", url });
    expect(first?.status).toBe(200);
    const etag = first?.headers.etag ?? "";
    const again = respond(world, BASE, { method: "GET", url, headers: { "if-none-match": etag } });
    expect(again?.status).toBe(304);
    expect(respond(world, BASE, { method: "GET", url: url.replace("algo", "nope") })).toBe(null);
  });

  it("serves over HTTP and records what was asked", async () => {
    const mock = await start(world);
    try {
      const anonymous = await fetch(`${mock.url}/graphql`, { method: "POST", body: "{}" });
      expect(anonymous.status).toBe(401);
      const answer = await fetch(`${mock.url}/graphql`, {
        method: "POST",
        headers: { authorization: "Bearer gho_x" },
        body: JSON.stringify({ query: LIST, variables: { cursor: null } }),
      });
      expect((await answer.json()).data.viewer.login).toBe("ada-e2e");
      expect(answer.headers.get("x-ratelimit-remaining")).toBe("4990");
      const avatar = await fetch(`${mock.url}/web/ada-e2e.png?size=64`);
      expect(avatar.headers.get("content-type")).toBe("image/png");
      expect((await fetch(`${mock.url}/users/ada-e2e`)).status).toBe(200);
      expect((await fetch(`${mock.url}/nothing`)).status).toBe(404);
      const asked = await (await fetch(`${mock.url}/__requests`)).json();
      expect(asked.map((r) => r.kind ?? r.path)).toEqual([
        "/graphql",
        "list",
        "/web/ada-e2e.png",
        "/users/ada-e2e",
        "/nothing",
      ]);
    } finally {
      await mock.close();
    }
  });
});
