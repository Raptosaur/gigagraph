const axios = require("axios");

async function ping(host) {
  const response = await axios.get(host);
  return response.status;
}

ping("http://localhost:3000");
