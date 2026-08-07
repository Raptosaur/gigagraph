exports.create = async (event) => {
  const body = JSON.parse(event.body);
  return { statusCode: 201, body: JSON.stringify({ id: 1, ...body }) };
};
