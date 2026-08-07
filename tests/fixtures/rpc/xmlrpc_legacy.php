<?php

function addNumbers($method, $args)
{
    return $args[0] + $args[1];
}

$srv = xmlrpc_server_create();
xmlrpc_server_register_method($srv, 'addNumbers', 'addNumbers');
