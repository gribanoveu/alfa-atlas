import { describe, expect, test } from "bun:test";
import { skeletonForSchema } from "../components/OpenApiExplorer/requestBuilder";

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
