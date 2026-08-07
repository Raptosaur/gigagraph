namespace Api;

public class UsersController
{
    [HttpGet("/api/users/{id}")]
    public string Get(int id) => "";
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
