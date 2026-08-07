// NodejsFunction entry target: the `entry` prop points at THIS file and the
// `handler` prop names the export.
export async function main(event: { path: string }) {
  return { statusCode: 200, body: JSON.stringify({ path: event.path }) };
}
