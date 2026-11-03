import { describe, expect, it } from "vitest";
import { parseDeepLink } from "./deeplink";

describe("parseDeepLink", () => {
  it("parses cluster links with view and query", () => {
    expect(parseDeepLink("manifex://cluster/abc-123/pods?namespace=prod&kind=pods&name=web-1")).toEqual({
      clusterId: "abc-123",
      view: "pods",
      namespace: "prod",
      kind: "pods",
      name: "web-1",
    });
  });

  it("parses view links", () => {
    expect(parseDeepLink("manifex://view/settings")).toEqual({ view: "settings" });
  });

  it("rejects unknown views, schemes and unsafe values", () => {
    expect(parseDeepLink("manifex://view/evil")).toBeNull();
    expect(parseDeepLink("https://cluster/abc")).toBeNull();
    expect(parseDeepLink("manifex://cluster/a b")).toBeNull();
    expect(parseDeepLink("not a url")).toBeNull();
    expect(parseDeepLink("manifex://cluster/abc?namespace=<script>")).toEqual({ clusterId: "abc" });
  });
});
