using System;
using System.Web.Services;

namespace Legacy
{
    public class QuoteService : WebService
    {
        [WebMethod]
        public double GetQuoteAsmx(string symbol)
        {
            return 42.0;
        }
    }
}
