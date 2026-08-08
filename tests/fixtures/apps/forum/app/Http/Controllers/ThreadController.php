<?php

namespace App\Http\Controllers;

use App\Services\ThreadService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class ThreadController extends Controller
{
    public function __construct(private ThreadService $threads)
    {
    }

    public function index(Request $request): JsonResponse
    {
        return response()->json($this->threads->recent((int) $request->query('limit', 20)));
    }

    public function show(string $slug): JsonResponse
    {
        $thread = $this->threads->bySlug($slug);
        return $thread === null
            ? response()->json(['error' => 'not found'], 404)
            : response()->json($thread);
    }

    public function store(Request $request): JsonResponse
    {
        $thread = $this->threads->open($request->input('title'), $request->input('body'));
        return response()->json($thread, 201);
    }

    public function destroy(string $slug): JsonResponse
    {
        $this->threads->close($slug);
        return response()->json(null, 204);
    }
}
