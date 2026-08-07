<?php

namespace App\Bundle\Controller;

use Sensio\Bundle\FrameworkExtraBundle\Configuration\Method;
use Sensio\Bundle\FrameworkExtraBundle\Configuration\Route;

class LegacyController
{
    /**
     * @Route("/reports/archive/{year}", name="legacy_archive")
     * @Method({"GET", "HEAD"})
     */
    public function archiveAction($year)
    {
        return '';
    }

    /**
     * @Route("/reports/export", methods={"POST"})
     */
    public function exportAction()
    {
        return '';
    }
}
