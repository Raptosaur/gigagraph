export const handler = async (event: unknown) => {
  return { statusCode: 200, body: JSON.stringify({ ok: true, event }) };
};
