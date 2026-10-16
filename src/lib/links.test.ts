import { describe, expect, it } from "vitest";
import type { Project, Unaccepted } from "$lib/api";
import { acceptLink, openLink } from "$lib/links";

const item = (secret: string | null) =>
  ({
    classroom: { org: "HEIG-VD", slug: "26-poo-c", secret },
    assignment: { slug: "labo-3" },
  }) as Unaccepted;

describe("links", () => {
  it("builds accept links, with the classroom key when it has one", () => {
    expect(acceptLink(item(null))).toBe("classroom50-toolbox://accept/HEIG-VD/26-poo-c/labo-3");
    expect(acceptLink(item("ab12"))).toBe(
      "classroom50-toolbox://accept/HEIG-VD/26-poo-c/labo-3?k=ab12",
    );
  });

  it("builds open links for projects on GitHub only", () => {
    const project = (owner: string | null, repo: string | null) => ({ owner, repo }) as Project;
    expect(openLink(project("heig-vd", "26-poo-c-labo-3-alice"))).toBe(
      "classroom50-toolbox://open/heig-vd/26-poo-c-labo-3-alice",
    );
    expect(openLink(project(null, null))).toBeNull();
  });
});
