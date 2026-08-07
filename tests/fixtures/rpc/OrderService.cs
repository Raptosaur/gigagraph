using System;
using System.ServiceModel;

namespace Legacy
{
    [ServiceContract]
    public class OrderService
    {
        [OperationContract]
        public string GetOrder(int id)
        {
            return "order-" + id;
        }

        public string NotExposed()
        {
            return "internal";
        }
    }
}
