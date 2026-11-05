import { describe, expect, it } from "vitest";
import { splitImage } from "./ImageEditor";

describe("splitImage", () => {
  it("separates repository and tag", () => {
    expect(splitImage("nginx:1.27")).toEqual({ repo: "nginx", tag: "1.27" });
    expect(splitImage("registry.local:5000/team/api:2.4.1")).toEqual({ repo: "registry.local:5000/team/api", tag: "2.4.1" });
    expect(splitImage("ghcr.io/x/y:1.0@sha256:abc")).toEqual({ repo: "ghcr.io/x/y", tag: "1.0" });
    expect(splitImage("registry.local:5000/team/api")).toEqual({ repo: "registry.local:5000/team/api", tag: "latest" });
  });
});
