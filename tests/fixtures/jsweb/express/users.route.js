// hagopj13/node-express-boilerplate shapes: router.route() verb chains and
// dotted controller handlers resolved through the require import.
const express = require('express');
const userController = require('./users.controller');

const router = express.Router();

router
  .route('/')
  .get(auth('getUsers'), userController.list)
  .post(userController.create);

router.route('/:id').delete(userController.remove);

module.exports = router;
