using System;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Runner.Server.Controllers
{
    public class WorkflowObjectConverter : JsonConverter
    {
        public override bool CanWrite => false;

        public override bool CanConvert(Type objectType) => true;

        public override object ReadJson(JsonReader reader, Type objectType, object existingValue, JsonSerializer serializer)
        {
            if (reader.TokenType != JsonToken.StartObject)
            {
                reader.Skip();
                return null;
            }
            var workflow = Activator.CreateInstance(objectType);
            serializer.Populate(JObject.Load(reader).CreateReader(), workflow);
            return workflow;
        }

        public override void WriteJson(JsonWriter writer, object value, JsonSerializer serializer)
        {
            throw new NotSupportedException();
        }
    }
}
