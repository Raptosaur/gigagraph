namespace Api;

public class UsersController
{
    [HttpGet("/api/users/{id}")]
    public string Get(int id) => "";
}

[Route("api/[controller]")]
public class GadgetInventoryController
{
    [HttpGet]
    public string List() => "";

    [HttpGet("{id}")]
    public string Find(int id) => "";

    [HttpPost("bulk")]
    public string Bulk() => "";

    [Route("export")]
    public string Export() => "";
}

[Route("api/[controller]/[action]")]
public class BaseToolController
{
}

public class WrenchController : BaseToolController
{
    [HttpGet]
    public string Sizes() => "";
}

public class Boot
{
    public void Configure(object app)
    {
        Wire(app);
    }

    private void Wire(object app)
    {
    }
}
