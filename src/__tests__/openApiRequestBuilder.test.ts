import { describe, expect, test } from "bun:test";
import {
  buildCurl,
  buildRequest,
  scalarSkeleton,
  skeletonForSchema,
} from "../components/OpenApiExplorer/requestBuilder";
import { parseParameters } from "../components/OpenApiExplorer/openApiModel";

describe("skeletonForSchema", () => {
  test("keeps the schema shape when an object has an incompatible scalar example", () => {
    expect(
      skeletonForSchema({
        type: "object",
        example: 56000,
        properties: {
          amount: { type: "integer", example: 1235 },
          currency: {
            type: "object",
            properties: {
              code: { type: "integer", example: 810 },
            },
          },
        },
      }),
    ).toEqual({
      amount: 1235,
      currency: { code: 810 },
    });
  });

  test("still uses a compatible explicit example", () => {
    expect(skeletonForSchema({ type: "integer", example: 56000 })).toBe(56000);
  });
});

describe("scalarSkeleton with fallbackExample", () => {
  test("uses fallback example if provided on parameter", () => {
    expect(scalarSkeleton({ type: "string" }, "XAAAAA")).toBe("XAAAAA");
    expect(scalarSkeleton({ type: "array", items: { type: "string" } }, "UAAAAA")).toBe("UAAAAA");
    expect(scalarSkeleton({ type: "integer" }, 810)).toBe("810");
  });

  test("falls back to schema example if parameter fallback not present", () => {
    expect(scalarSkeleton({ type: "string", example: "FRONT" })).toBe("FRONT");
    expect(scalarSkeleton({ type: "string" })).toBe("");
  });
});

describe("parseParameters", () => {
  test("extracts parameter example along with schema and metadata", () => {
    const params = parseParameters({
      parameters: [
        {
          name: "A-userId",
          in: "header",
          required: true,
          description: "xPin",
          schema: { type: "string" },
          example: "XAAAAA",
        },
      ],
    });

    expect(params).toEqual([
      {
        name: "A-userId",
        in: "header",
        required: true,
        description: "xPin",
        schema: { type: "string" },
        example: "XAAAAA",
      },
    ]);
  });
});

describe("buildRequest and buildCurl with corporate headers", () => {
  test("includes corporate headers in built request and curl command", () => {
    const request = buildRequest({
      baseUrl: "https://api.example.com",
      path: "/v1/cards",
      method: "get",
      paramValues: {
        "header:A-userId": "XAAAAA",
        "header:A-customerId": "UAAAAA",
        "header:A-channelId": "NIB",
      },
      paramEntries: [
        { name: "A-userId", in: "header" },
        { name: "A-customerId", in: "header" },
        { name: "A-channelId", in: "header" },
      ],
      bodyMediaType: null,
      bodyText: "",
      hasBody: false,
    });

    expect(request.headers).toEqual({
      "A-userId": "XAAAAA",
      "A-customerId": "UAAAAA",
      "A-channelId": "NIB",
    });

    const curl = buildCurl(request);
    expect(curl).toContain("curl -X GET 'https://api.example.com/v1/cards'");
    expect(curl).toContain("-H 'A-userId: XAAAAA'");
    expect(curl).toContain("-H 'A-customerId: UAAAAA'");
    expect(curl).toContain("-H 'A-channelId: NIB'");
  });
});
