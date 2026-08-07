const express = require('express');
const apiRoutes = require('./api');

const app = express();
app.use('/api/v1', apiRoutes);

module.exports = app;
