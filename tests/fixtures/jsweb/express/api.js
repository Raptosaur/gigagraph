// Mid-level router: mounts the users router, and is itself mounted by app.js
// — the composed prefix must reach users.route.js transitively.
const express = require('express');
const usersRoute = require('./users.route');

const router = express.Router();
router.use('/users', usersRoute);

module.exports = router;
